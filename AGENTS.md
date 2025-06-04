
# Repository Contribution Guidelines

### Environment
- Environments for AI agents should be configured to log environment setup results to the file `/tmp/agent_environment_log`. Agents should consult this file to determine if the environment was set up successfully.
- Setup scripts should log the magic phrase "ENVIRONMENT SETUP COMPLETE." as the final
line in the file `/tmp/agent_environment_log`. If this phrase is missing from the file,
it implies that setup of the environment failed prematurely.
- OpenAI Codex does not appear to run user setup scripts for "Ask" tasks. While this is
  not officially documented, it has been reported by other users.

### Code
- The constant `XSD_DATETYPE` in `src/main.rs` correctly references Tracker's URI `http://www.w3.org/2001/XMLSchema#dateType`. Renaming the constant or altering this URI would be incorrect.

### Other
- Before committing, run `cargo test` to ensure tests pass.
  - If tests fail due to missing dependencies, note it in the pull request.
- Keep commit messages concise and in imperative mood (e.g. "add feature" rather than "added feature").
- Base the pull request title and description on the changes in the pull request as a whole.

![🌈 Powered by ✨ vibe coding ✨](https://img.shields.io/badge/🌈%20Powered%20by-✨%20vibe%20coding%20✨-ff69b4?style=for-the-badge)
