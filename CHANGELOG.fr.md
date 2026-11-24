## 0.1.0 · 2026-11-24

- Première version : à la fin de chaque tour d’agent, php -l, PHPStan, PHP-CS-Fixer, lint YAML et Twig, composer validate et ESLint sur les fichiers que l’agent a modifiés, chacun avec la configuration du projet, dans Docker.
- Une pastille verte ou rouge sur la ligne de l’agent, le détail dans le panneau Contrôles, et Demander à l’agent de corriger.
- Les agents peuvent lancer les contrôles eux-mêmes avec `checks_run`.
