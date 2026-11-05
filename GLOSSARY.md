# Small Streamer Chat

A free desktop tool that gives new streamers an AI-generated chat to talk to while they have few or no viewers. It listens to the streamer and shows a simulated chat, without ever connecting to a streaming platform.

## Language

### The chat

**Practice Chat**:
The simulated chat feed the tool produces. It exists only on the streamer's machine and is never posted to a platform.
_Avoid_: Fake chat, bot chat, AI chat

**AI Chatter**:
A named persona with a fixed username and colour that writes messages in the Practice Chat, always marked as AI.
_Avoid_: Bot, fake viewer, viewer

**Persona**:
The personality and style definition an AI Chatter speaks from. The tool ships with built-in ones, and the streamer can edit or add more.
_Avoid_: Character, profile

**AI Badge**:
The required marker on every AI Chatter message identifying it as AI. Its style can change, but it can't be removed.
_Avoid_: Label, tag, disclaimer

**Content Level**:
How edgy AI Chatters may be: family-friendly, normal teasing, or spicy. Slurs, sexual content and harassment are blocked at every level.
_Avoid_: Filter, safety mode, spice

**Overlay**:
The Practice Chat rendered for capture by streaming software (e.g. an OBS browser source), styled like a platform's native chat.
_Avoid_: Widget, embed

### The streamer and the session

**Streamer**:
The person using the tool and broadcasting live.
_Avoid_: User, host, creator

**Session**:
One run of the Practice Chat, from start to stop. Everything remembered during a Session is discarded when it ends.
_Avoid_: Stream, run

**Stream Context**:
The streamer's free-text description of what they're doing (game, topic, vibe), given to AI Chatters alongside what the streamer says.
_Avoid_: Prompt, description, topic

**Stream Memory**:
The running summary of what has happened so far in the Session, which lets AI Chatters make callbacks.
_Avoid_: History, log

**Nudge**:
The streamer asking for a Practice Chat message right now instead of waiting.
_Avoid_: Trigger, force

**Back-off**:
The streamer manually quieting the Practice Chat, typically because Real Chatters have arrived.
_Avoid_: Pause, mute

**AI Connection**:
How the streamer pays for the AI behind the Practice Chat: an API key, or signing in with a subscription plan that allows third-party apps.
_Avoid_: Login, account, provider (when you mean the connection)

### Outside the tool

**Real Chat**:
The actual chat on the streamer's platform channel. The tool neither reads nor writes it.
_Avoid_: Live chat, platform chat

**Real Chatter**:
A human who posts in the Real Chat. AI Chatters never respond to or acknowledge them.
_Avoid_: Viewer, user
