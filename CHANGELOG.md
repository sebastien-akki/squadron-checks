## 0.1.0 · 2026-11-24

- First version: at the end of each agent turn, php -l, PHPStan, PHP-CS-Fixer, YAML and Twig lint, composer validate and ESLint on the files the agent changed, each with the project's own config, in Docker.
- A green or red chip on the agent's row, the details in the Checks panel, and Ask the agent to fix.
- Agents can run the checks themselves with `checks_run`.
