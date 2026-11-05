# AI connections: an Anthropic API key, or sign in with a ChatGPT plan

The MVP offers two ways to pay for AI: an Anthropic API key (Claude Haiku), or signing in with a ChatGPT plan. The two providers are treated differently because of their rules. Since February 2026, Anthropic only allows Claude subscription logins inside Claude.ai and Claude Code, so Claude needs an API key. OpenAI's "Sign in with ChatGPT" (launched September 2026) explicitly supports open-source and locally hosted apps using a user's ChatGPT plan, with no partner approval or client secret ([docs](https://developers.openai.com/siwc/token-sharing-open-source)). Paid or hosted apps need OpenAI's interest form, so this option depends on the tool staying free, open source and local.

## Consequences

- The ChatGPT flow has to follow OpenAI's rules: a PKCE loopback redirect on `127.0.0.1` (not `localhost`), the `chatgpt.tokens.use.direct` scope, only the public `api.openai.com/v1/responses` endpoint with `store:false` and `stream:true`, and a "Continue with ChatGPT" button following OpenAI's branding guidelines.
- Usage comes out of the streamer's ChatGPT plan allowance, and can fail partway through a reply with `subscription_sharing_usage_limit_exceeded`. The Practice Chat has to handle that gracefully.
- A Claude Code CLI route stays rejected (see the Q8 decision in the design session).
