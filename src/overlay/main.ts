// The OBS Overlay: a standalone page served by the Rust core's local server.
// It receives Practice Chat messages over Server-Sent Events.

import "../shared/chat.css";
import "./overlay.css";
import type { ChatMessage } from "../shared/chat";

/** Most lines shown at once. */
const MAX_LINES = 12;
/** Seconds before a line fades out. */
const LINE_LIFETIME_S = 45;

const chat = document.getElementById("chat")!;

// In dev the page comes from Vite, so the server address is passed in the URL.
const server = new URLSearchParams(location.search).get("server") ?? location.origin;

function addLine(message: ChatMessage) {
  const line = document.createElement("div");
  line.className = "chat-line";

  const badge = document.createElement("span");
  badge.className = "ai-badge";
  badge.textContent = "AI";

  const chatter = document.createElement("span");
  chatter.className = "chatter";
  chatter.style.color = message.color;
  chatter.textContent = message.chatter;

  const text = document.createElement("span");
  text.className = "text";
  text.textContent = `: ${message.text}`;

  line.append(badge, chatter, text);
  chat.append(line);

  while (chat.childElementCount > MAX_LINES) chat.firstElementChild!.remove();

  setTimeout(() => {
    line.classList.add("fading");
    line.addEventListener("transitionend", () => line.remove(), { once: true });
  }, LINE_LIFETIME_S * 1000);
}

// EventSource reconnects on its own if the app restarts.
const events = new EventSource(new URL("/events", server));
events.addEventListener("chat-message", (event) => {
  addLine(JSON.parse((event as MessageEvent<string>).data));
});
