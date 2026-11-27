<script lang="ts">
  import type { GreetdSession, GreetdState } from "@webbar/types";
  import { fromExternalStore } from "webbar-sway-full-bar-svelte/webbarStore";

  const FALLBACK_SESSION: GreetdSession = {
    id: "wayland:sway",
    name: "Sway",
    command: ["sway"],
    kind: "wayland",
  };

  const greetdStore = fromExternalStore(
    window.webbar.greetd.subscribe,
    window.webbar.greetd.get,
  );

  let username = $state("");
  let response = $state("");
  let chosenSessionId = $state<string | null>(null);
  let busy = $state(false);
  let localError = $state<string | null>(null);
  let userInputEl = $state<HTMLInputElement | null>(null);
  let responseInputEl = $state<HTMLInputElement | null>(null);

  let greetd = $derived(greetdStore.current);
  let sessions = $derived(
    greetd.sessions.length > 0 ? greetd.sessions : [FALLBACK_SESSION],
  );
  let selectedSession = $derived(
    sessions.find((s) => s.id === chosenSessionId) ?? sessions[0],
  );
  let authenticating = $derived(greetd.phase === "authenticating");
  let readyToLaunch = $derived(greetd.phase === "ready_to_launch");
  let launching = $derived(
    greetd.phase === "launching" || greetd.phase === "launched",
  );
  let inSession = $derived(authenticating || readyToLaunch);
  let prompt = $derived(greetd.prompt);
  let needsTextInput = $derived(
    authenticating &&
      prompt !== null &&
      (prompt.kind === "secret" || prompt.kind === "visible"),
  );
  let errorMessage = $derived(localError ?? greetd.error);
  let statusMessage = $derived.by(() => {
    if (!greetd.available) {
      return "Waiting for greetd socket (GREETD_SOCK)…";
    }
    if (
      authenticating &&
      prompt &&
      (prompt.kind === "info" || prompt.kind === "error")
    ) {
      return prompt.message;
    }
    if (greetd.phase === "launching") {
      return "Starting session…";
    }
    if (greetd.phase === "launched") {
      return "Session started.";
    }
    return null;
  });

  $effect(() => {
    if (needsTextInput && responseInputEl) {
      const el = responseInputEl;
      window.requestAnimationFrame(() => el.focus());
    } else if (!inSession && !launching && greetd.available && userInputEl) {
      const el = userInputEl;
      window.requestAnimationFrame(() => el.focus());
    }
  });

  async function handleSubmit(ev: SubmitEvent): Promise<void> {
    ev.preventDefault();
    if (busy) return;
    const state = window.webbar.greetd.get();
    if (!state.available) return;

    busy = true;
    localError = null;

    try {
      let next: GreetdState;
      if (state.phase === "inactive") {
        next = await window.webbar.greetd.createSession(username.trim());
      } else if (state.phase === "authenticating") {
        const currentPrompt = state.prompt;
        const val =
          currentPrompt &&
          (currentPrompt.kind === "secret" || currentPrompt.kind === "visible")
            ? response
            : null;
        response = "";
        next = await window.webbar.greetd.respond(val);
      } else {
        next = state;
      }

      if (next.phase === "ready_to_launch") {
        await window.webbar.greetd.launch(selectedSession.command);
      }
    } catch (err) {
      response = "";
      localError = err instanceof Error ? err.message : String(err);
    } finally {
      busy = false;
    }
  }

  async function handleCancel(): Promise<void> {
    if (busy) return;
    busy = true;
    localError = null;
    response = "";
    try {
      await window.webbar.greetd.cancelSession();
    } catch (err) {
      localError = err instanceof Error ? err.message : String(err);
    } finally {
      busy = false;
      userInputEl?.focus();
    }
  }
</script>

<section class="login-card" aria-label="Sign in">
  <header class="login-header">
    <h1 class="login-title">Sign in</h1>
    {#if greetd.user && inSession}
      <p class="login-user">{greetd.user}</p>
    {/if}
  </header>

  {#if statusMessage}
    <p id="status" class="login-status" role="status">{statusMessage}</p>
  {/if}
  {#if errorMessage}
    <p id="error" class="login-error" role="alert">{errorMessage}</p>
  {/if}

  <form
    id="login-form"
    class="login-form"
    autocomplete="off"
    onsubmit={handleSubmit}
  >
    <label id="user-field" class="login-field" hidden={inSession || launching}>
      <span>Username</span>
      <input
        bind:this={userInputEl}
        bind:value={username}
        id="username"
        name="username"
        type="text"
        autocomplete="username"
        placeholder="alice"
        required={!inSession && !launching}
      />
    </label>

    <label id="prompt-field" class="login-field" hidden={!needsTextInput}>
      <span id="prompt-label">{prompt?.message || "Password"}</span>
      <input
        bind:this={responseInputEl}
        bind:value={response}
        id="response"
        name="response"
        type={prompt?.kind === "secret" ? "password" : "text"}
        autocomplete="current-password"
        required={needsTextInput}
      />
    </label>

    <label id="session-field" class="login-field">
      <span>Session</span>
      <select
        id="session"
        name="session"
        value={selectedSession.id}
        disabled={!greetd.available || busy || launching}
        required
        onchange={(e) => {
          chosenSessionId = e.currentTarget.value;
        }}
      >
        {#each sessions as item (item.id)}
          <option value={item.id}>
            {item.kind === "x11" ? `${item.name} (X11)` : item.name}
          </option>
        {/each}
      </select>
    </label>

    <div class="login-actions">
      <button
        id="cancel-btn"
        type="button"
        class="login-btn secondary"
        hidden={!inSession}
        disabled={busy}
        onclick={handleCancel}
      >
        Cancel
      </button>
      <button
        id="submit-btn"
        type="submit"
        class="login-btn primary"
        disabled={!greetd.available || busy || launching}
      >
        {inSession ? "Sign in" : "Continue"}
      </button>
    </div>
  </form>
</section>

<style>
  .login-card {
    width: min(380px, 100%);
    padding: 24px;
    border-radius: var(--radius-lg);
    background: var(--panel-bg);
    color: var(--fg-color);
    border: 1px solid var(--panel-border);
    box-shadow:
      0 16px 48px var(--shadow-lg),
      0 2px 8px var(--shadow-sm);
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .login-header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }

  .login-title {
    margin: 0;
    font-size: 18px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  .login-user {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent-color);
    background: var(--accent-soft);
    padding: 3px 9px;
    border-radius: 999px;
  }

  .login-form {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .login-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .login-field[hidden] {
    display: none;
  }

  .login-field span {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--fg-muted);
  }

  .login-field input,
  .login-field select {
    width: 100%;
    padding: 10px 12px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--panel-border);
    background: var(--surface-color);
    color: var(--fg-color);
    font: inherit;
    font-weight: 500;
    user-select: text;
    transition:
      border-color var(--dur-fast) var(--ease-out),
      background-color var(--dur-fast) var(--ease-out);
  }

  .login-field input::placeholder {
    color: var(--fg-faint);
  }

  .login-field input:focus,
  .login-field select:focus {
    outline: 2px solid var(--accent-color);
    outline-offset: 1px;
    background: var(--surface-hover);
  }

  .login-field select option {
    background: var(--panel-bg);
    color: var(--fg-color);
  }

  .login-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }

  .login-btn {
    height: 36px;
    padding: 0 16px;
    border-radius: var(--radius-sm);
    font-size: 13px;
    font-weight: 600;
  }

  .login-btn[hidden] {
    display: none;
  }

  .login-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .login-btn.primary {
    background: var(--accent-color);
    color: var(--accent-fg);
  }

  .login-btn.secondary {
    background: var(--surface-color);
    color: var(--fg-color);
  }

  .login-btn.secondary:hover:not(:disabled) {
    background: var(--surface-hover);
  }

  .login-status,
  .login-error {
    margin: 0;
    padding: 9px 12px;
    border-radius: var(--radius-sm);
    font-size: 12.5px;
    font-weight: 500;
  }

  .login-status {
    background: var(--accent-soft);
    color: var(--fg-color);
  }

  .login-error {
    background: var(--red-soft);
    color: var(--accent-red);
  }
</style>
