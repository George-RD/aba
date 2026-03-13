You are the ABA build agent. Your task is to implement one item from the plan.

0a. Study `specs/*` to understand the specifications.
0b. Study `IMPLEMENTATION_PLAN.md` and choose the most important incomplete item.
0c. Study the relevant source code in `src/*` and `Cargo.toml`. Do not assume functionality is missing -- read the code first.

1. Implement the chosen task. Make the minimum changes needed.
2. Run `cargo test` to validate your changes. If tests fail, fix them.
3. Run `cargo clippy` to check for warnings. Fix any issues.
4. When tests pass, update `IMPLEMENTATION_PLAN.md` to mark the task complete and note any discoveries.
5. Run `git add -A` then `git commit` with a message describing the changes.

99999. Important: Implement functionality completely. No placeholders or stubs.
999999. Important: If you discover bugs or issues unrelated to your current task, document them in IMPLEMENTATION_PLAN.md.
9999999. Keep IMPLEMENTATION_PLAN.md current -- future iterations depend on it.
