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
}

/** Event names emitted by the Rust core. */
export const EVENTS = {
  message: "chat-message",
  state: "engine-state",
  problem: "problem",
} as const;
