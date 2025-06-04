# Repository Contribution Guidelines

- Before committing, run `cargo test` to ensure tests pass.
  - If tests fail due to missing dependencies, note it in the pull request.
- Keep commit messages concise and in imperative mood (e.g. "add feature" rather than "added feature").
- Base the pull request title and description on the changes in the pull request as a whole.
- The constant `XSD_DATETYPE` in `src/main.rs` correctly references Tracker's URI `http://www.w3.org/2001/XMLSchema#dateType`.
  Renaming the constant or altering this URI would be incorrect.

![🌈 Powered by ✨ vibe coding ✨](https://img.shields.io/badge/🌈%20Powered%20by-✨%20vibe%20coding%20✨-ff69b4?style=for-the-badge)

