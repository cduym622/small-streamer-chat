# The Practice Chat never posts to a streaming platform

The tool only shows AI messages on the streamer's own machine (in its own window or as an OBS Overlay). It never sends them to Twitch, YouTube or TikTok chat. Posting AI messages from linked accounts counts as fake engagement under Twitch's and YouTube's rules and could get the streamer banned. TikTok has no official API for sending chat at all. Keeping everything on the streamer's machine also means one design works on every platform. Reading the Real Chat (read-only, for Back-off) is allowed.

## Considered Options

- **Several linked platform accounts posting as viewers**: rejected. Twitch and YouTube treat this as fake engagement, with suspension or termination as the penalty.
- **One disclosed bot account posting to Twitch chat**: allowed under Twitch's rules, but Twitch-only and adds account linking. Rejected to keep the tool platform-agnostic and free of any platform login.
