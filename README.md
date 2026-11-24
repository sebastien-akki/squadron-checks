# Checks

Quality checks on the files each Squadron agent changed, at the end of its turn, with a green or red chip on the agent's row.

## Use

- When an agent finishes a turn and changed something since the last check, the checks run on the files its branch changed: against where it left the main branch (`origin/HEAD`, `origin/main` or `origin/master`), plus what is not committed. `vendor/`, `node_modules/`, `var/` and `public/build/` are left out.
- Each check runs only when the project uses the tool, with the project's own config:
  - **php -l** on PHP files;
  - **PHPStan** with `phpstan.neon`, `phpstan.neon.dist` or `phpstan.dist.neon`;
  - **PHP-CS-Fixer** (dry run) with `.php-cs-fixer.php` or `.php-cs-fixer.dist.php`, from `vendor/bin` or `tools/php-cs-fixer`;
  - **YAML** with Symfony's `vendor/bin/yaml-lint`;
  - **Twig** with `bin/console lint:twig`;
  - **composer validate** when `composer.json` or `composer.lock` changed;
  - **ESLint** with the project's ESLint and config.
- The agent's row gets a chip: the number of problems in red, or "Checks OK" in green. Its tooltip lists each check.
- **Checks** in the agent's toolbar opens the panel: each check, its problems as `file:line`, what a check that could not run said, **Run again**, and **Ask the agent to fix**, which puts the list of problems in your message.
- Agents can call `checks_run` themselves before saying they are done. Claude Code asks you first.
- Settings: checks at the end of each turn (on by default), the PHP image, the Node image.

## Docker images

Each check runs in a container with the worktree mounted at `/app`, without network, as your user so that what it writes stays yours. The PHP version comes from `.lando.yml`, then `composer.json` (`config.platform.php`, then `require.php`). When Docker already has Lando's `devwithlando/php` image of that version, it is used, since it has the extensions Symfony's console needs; otherwise `php:<version>-cli`, downloaded the first time. `composer validate` runs in `composer:2`, ESLint in `node:22`. A PHPStan or PHP-CS-Fixer that is not installed in `vendor/`, for example in a fresh worktree, is simply not run.

## What it asks Squadron for

- **Run `git`**: to find the files the branch changed and the project's PHP version.
- **Run `docker`**: to run each check in a container, without network.
- **Run `id`**: to run the containers as you.

No network access of its own and no secret. Results stay in the extension's storage, for the last 40 agents.

## Build

```sh
rustup target add wasm32-wasip2
cargo test
cargo build --release --target wasm32-wasip2
```

Then in Squadron: Settings › Extensions › Add an extension › A folder on this computer.
