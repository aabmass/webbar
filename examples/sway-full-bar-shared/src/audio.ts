import type { AudioDevice, AudioState } from "@webbar/types";

export function getActiveSpeaker(
  audioState: AudioState,
): AudioDevice | undefined {
  return audioState.outputs.find((o) => o.default) ?? audioState.outputs[0];
}

export function getActiveMic(audioState: AudioState): AudioDevice | undefined {
  return audioState.inputs.find((i) => i.default) ?? audioState.inputs[0];
}

export function sliderMaxForVolume(volume: number): number {
  return volume > 1 ? 1.5 : 1;
}

export function fillPercentForVolume(volume: number): number {
  const max = sliderMaxForVolume(volume);
  return Math.min(100, (volume / max) * 100);
}

export function openAudioMixer(): void {
  window.webbar.sway.command("exec pwvucontrol").catch(() => {
    void window.webbar.exec(["pwvucontrol"]).catch(() => {});
  });
}
