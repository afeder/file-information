
# Repository Contribution Guidelines

### Environment
- Environments for AI agents should be configured to log environment setup results to the file `/tmp/agent_environment_log`.
- Setup scripts should log the magic phrase "ENVIRONMENT SETUP COMPLETE." as the final
line in the file `/tmp/agent_environment_log`. If this phrase is missing from the file,
it implies that setup of the environment failed prematurely.
- If agents encounter unexpectedly missing packages, they should consult this file to determine if the environment was set up successfully. If it wasn't, they should communicate
it to the user.
- OpenAI Codex does not appear to run user setup scripts for "Ask" tasks. While this is
  not officially documented, it has been reported by other users.

### Code
- The constant `XSD_DATETYPE` in `src/main.rs` correctly references the URI `http://www.w3.org/2001/XMLSchema#dateType` used by Tracker. Renaming the constant or altering this URI would be incorrect.

### Other
- When altering `src/main.rs`, add as many detailed, instructive, and explanatory comments as possible. It's better to include more comments, with plenty of detail, rather than too few—any unnecessary comments can be easily reviewed and removed later.
- Before committing, run `tests/run_tests.sh` to ensure tests pass.
  - If tests fail, note it in the pull request.
- Running `tests/run_tests.sh` may take a considerable amount of time because it
has to compile large libraries like GTK.
- Keep commit messages concise and in imperative mood (e.g. "add feature" rather than "added feature").

![🌈 Powered by ✨ vibe coding ✨](https://img.shields.io/badge/🌈%20Powered%20by-✨%20vibe%20coding%20✨-ff69b4?style=for-the-badge)
