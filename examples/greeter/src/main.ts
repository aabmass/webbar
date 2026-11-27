import type { GreetdSession, GreetdState } from "@webbar/types";

const FALLBACK_SESSIONS: GreetdSession[] = [
  {
    id: "wayland:sway",
    name: "Sway",
    command: ["sway"],
    kind: "wayland",
  },
];

const clockEl = document.getElementById("clock")!;
const dateEl = document.getElementById("date")!;
const statusEl = document.getElementById("status")!;
const errorEl = document.getElementById("error")!;
const form = document.getElementById("login-form") as HTMLFormElement;
const userField = document.getElementById("user-field")!;
const userInput = document.getElementById("username") as HTMLInputElement;
const promptField = document.getElementById("prompt-field")!;
const promptLabel = document.getElementById("prompt-label")!;
const responseInput = document.getElementById("response") as HTMLInputElement;
const sessionSelect = document.getElementById("session") as HTMLSelectElement;
const cancelBtn = document.getElementById("cancel-btn") as HTMLButtonElement;
const submitBtn = document.getElementById("submit-btn") as HTMLButtonElement;

function updateClock(): void {
  const now = new Date();
  clockEl.textContent = now.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
  dateEl.textContent = now.toLocaleDateString([], {
    weekday: "long",
    month: "long",
    day: "numeric",
  });
}
updateClock();
window.setInterval(updateClock, 1000);

let busy = false;
let localError: string | null = null;
let renderedSessions: GreetdSession[] | null = null;
let chosenSessionId: string | null = null;

sessionSelect.addEventListener("change", () => {
  chosenSessionId = sessionSelect.value;
});

function availableSessions(state: GreetdState): GreetdSession[] {
  return state.sessions.length > 0 ? state.sessions : FALLBACK_SESSIONS;
}

function sessionLabel(session: GreetdSession): string {
  return session.kind === "x11" ? `${session.name} (X11)` : session.name;
}

function syncSessions(state: GreetdState): void {
  const sessions = availableSessions(state);
  if (sessions === renderedSessions) return;
  renderedSessions = sessions;

  sessionSelect.replaceChildren(
    ...sessions.map((s) => new Option(sessionLabel(s), s.id)),
  );
  if (chosenSessionId && sessions.some((s) => s.id === chosenSessionId)) {
    sessionSelect.value = chosenSessionId;
  }
}

function render(state: GreetdState): void {
  syncSessions(state);
  const err = localError ?? state.error;
  errorEl.hidden = !err;
  errorEl.textContent = err ?? "";

  if (!state.available) {
    statusEl.hidden = false;
    statusEl.textContent = "Waiting for greetd socket (GREETD_SOCK)…";
    sessionSelect.disabled = true;
    submitBtn.disabled = true;
    cancelBtn.hidden = true;
    return;
  }

  const authenticating = state.phase === "authenticating";
  const readyToLaunch = state.phase === "ready_to_launch";
  const launching = state.phase === "launching" || state.phase === "launched";
  const inSession = authenticating || readyToLaunch;

  userField.hidden = inSession || launching;
  userInput.required = !inSession && !launching;

  const prompt = state.prompt;
  const needsTextInput =
    authenticating &&
    prompt !== null &&
    (prompt.kind === "secret" || prompt.kind === "visible");

  promptField.hidden = !needsTextInput;
  if (needsTextInput && prompt) {
    promptLabel.textContent = prompt.message || "Password";
    responseInput.type = prompt.kind === "secret" ? "password" : "text";
    responseInput.required = true;
    window.requestAnimationFrame(() => responseInput.focus());
  } else {
    responseInput.required = false;
  }

  if (authenticating && prompt && (prompt.kind === "info" || prompt.kind === "error")) {
    statusEl.hidden = false;
    statusEl.textContent = prompt.message;
  } else if (state.phase === "launching") {
    statusEl.hidden = false;
    statusEl.textContent = "Starting session…";
  } else if (state.phase === "launched") {
    statusEl.hidden = false;
    statusEl.textContent = "Session started.";
  } else {
    statusEl.hidden = true;
    statusEl.textContent = "";
  }

  sessionSelect.disabled = busy || launching;
  cancelBtn.hidden = !inSession;
  cancelBtn.disabled = busy;
  submitBtn.disabled = busy || launching;
  submitBtn.textContent = inSession ? "Sign in" : "Continue";
}

async function launchSelectedSession(): Promise<void> {
  const sessions = availableSessions(window.webbar.greetd.get());
  const selected =
    sessions.find((s) => s.id === sessionSelect.value) ?? sessions[0];
  await window.webbar.greetd.launch(selected.command);
}

form.addEventListener("submit", async (ev) => {
  ev.preventDefault();
  if (busy) return;
  const state = window.webbar.greetd.get();
  if (!state.available) return;

  busy = true;
  localError = null;
  render(state);

  try {
    let next: GreetdState;
    if (state.phase === "inactive") {
      next = await window.webbar.greetd.createSession(userInput.value.trim());
    } else if (state.phase === "authenticating") {
      const prompt = state.prompt;
      const val =
        prompt && (prompt.kind === "secret" || prompt.kind === "visible")
          ? responseInput.value
          : null;
      responseInput.value = "";
      next = await window.webbar.greetd.respond(val);
    } else {
      next = state;
    }

    if (next.phase === "ready_to_launch") {
      await launchSelectedSession();
    }
  } catch (err) {
    responseInput.value = "";
    localError = err instanceof Error ? err.message : String(err);
  } finally {
    busy = false;
    render(window.webbar.greetd.get());
  }
});

cancelBtn.addEventListener("click", async () => {
  if (busy) return;
  busy = true;
  localError = null;
  responseInput.value = "";
  render(window.webbar.greetd.get());
  try {
    await window.webbar.greetd.cancelSession();
  } catch (err) {
    localError = err instanceof Error ? err.message : String(err);
  } finally {
    busy = false;
    render(window.webbar.greetd.get());
    userInput.focus();
  }
});

window.webbar.greetd.onChange((state) => {
  render(state);
});
