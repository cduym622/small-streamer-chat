/** One Practice Chat message, as sent by the Rust engine. */
export interface ChatMessage {
  id: number;
  /** The AI Chatter's username. */
  chatter: string;
  /** The AI Chatter's username colour. */
  color: string;
  text: string;
}

export interface EngineState {
  running: boolean;
  backedOff: boolean;
  /** AI cost of the current (or last) Session, in US dollars. */
  sessionCost: number;
  /** The AI Connection writing the messages. */
  provider: Provider;
}

/** Which AI Connection writes the messages; "none" uses canned lines. */
export type Provider = "none" | "anthropic" | "chatGpt";

/** Where the streamer sees and limits this app's use of their ChatGPT plan. */
export const CHATGPT_USAGE_URL = "https://chatgpt.com/#settings/Usage";

export type ModelStatus =
  | { state: "checking" | "loading" | "ready" }
  | { state: "downloading"; percent: number }
  | { state: "failed"; error: string };

export interface HearingStatus {
  model: ModelStatus;
  /** The mic is open (a Session is running). */
  listening: boolean;
  /** The chosen mic's id; `null` follows the system default. */
  microphone: string | null;
  /** Name of the mic in use while listening. */
  microphoneName: string | null;
}

export interface Microphone {
  id: string;
  name: string;
}

/** One thing the streamer said, as heard by speech recognition. */
export interface TranscriptLine {
  id: number;
  text: string;
}

/** Event names emitted by the Rust core. */
export const EVENTS = {
  message: "chat-message",
  state: "engine-state",
  problem: "problem",
  hearingStatus: "hearing-status",
  transcript: "transcript",
  micLevel: "mic-level",
  aiStatus: "ai-status",
} as const;
