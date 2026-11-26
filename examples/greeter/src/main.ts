import type { GreetdState } from "@webbar/types";

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
const sessionInput = document.getElementById("session") as HTMLInputElement;
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

function parseCommand(raw: string): string[] {
  return raw
    .trim()
    .split(/\s+/)
    .filter((part) => part.length > 0);
}

function render(state: GreetdState): void {
  const err = localError ?? state.error;
  errorEl.hidden = !err;
  errorEl.textContent = err ?? "";

  if (!state.available) {
    statusEl.hidden = false;
    statusEl.textContent = "Waiting for greetd socket (GREETD_SOCK)…";
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

  cancelBtn.hidden = !inSession;
  cancelBtn.disabled = busy;
  submitBtn.disabled = busy || launching;
  submitBtn.textContent = inSession ? "Sign in" : "Continue";
}

async function launchSelectedSession(): Promise<void> {
  const cmd = parseCommand(sessionInput.value);
  if (cmd.length === 0) {
    localError = "Session command must not be empty";
    render(window.webbar.greetd.get());
    return;
  }
  await window.webbar.greetd.launch(cmd);
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
