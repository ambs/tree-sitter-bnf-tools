# Pull Requests

## Branch naming

| Type | Pattern | Use for |
|------|---------|---------|
| Feature | `feature/<slug>` | New functionality |
| Bug fix | `fix/<slug>` | Defect corrections |
| Chore | `chore/<slug>` | Tooling, config, restructuring |
| Docs | `docs/<slug>` | Documentation only |

Use kebab-case slugs, e.g. `chore/restructure`, `feature/corpus-tests`.

## Target branch

All PRs target `main`.

## Checklist before opening

- [ ] `make fmt-check` passes
- [ ] `make lint` passes
- [ ] `make test` passes
- [ ] `make test-grammar` passes (if grammar changed)
