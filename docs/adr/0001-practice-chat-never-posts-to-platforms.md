# The tool never connects to a streaming platform

The tool only shows AI messages on the streamer's own machine (in its own window or as an OBS Overlay). It never sends them to Twitch, YouTube or TikTok chat, and v1 doesn't read Real Chat either. Posting AI messages from linked accounts counts as fake engagement under Twitch's and YouTube's rules and could get the streamer banned. TikTok has no official API for sending chat at all. With no platform connection, one design works on every platform with no platform login. The cost is that Back-off is manual: the streamer presses a button or hotkey when Real Chatters arrive.

## Considered Options

- **Several linked platform accounts posting as viewers**: rejected. Twitch and YouTube treat this as fake engagement, with suspension or termination as the penalty.
- **One disclosed bot account posting to Twitch chat**: allowed under Twitch's rules, but Twitch-only and adds account linking.
- **Read-only Real Chat for automatic Back-off**: possible without a login on Twitch, but limited or unreliable on YouTube and TikTok. Deferred, not ruled out. It's cheap to add later for Twitch.
