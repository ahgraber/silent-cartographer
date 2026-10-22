# Dogfood: method-overrides

Run on 2026-10-21 with the working-tree release build, against the persistent clones in `~/.cache/silent-cartographer/`.
The comparison build is `main` at `c0337d7`.

## Flask

| Indexer           | `overrides` edges |
| ----------------- | ----------------- |
| scip-python 0.6.6 | 60                |
| ty-scip 0.1.0     | 60                |

The two edge sets are identical, compared as sorted `src -> dst` lists.
There is no difference to classify.

Examples of the edges: `Flask.__init__ -> App.__init__`, `App.add_url_rule -> Scaffold.add_url_rule`, and each `JSONTag` subclass's `check`, `to_json`, and `to_python` to the `JSONTag` method.
23 of the 60 edges start in test code.

The `references` answer for `flask.json.tag.JSONTag.to_json` has 2 results and this line:

```text
overridden by 9 methods, counting overrides of overrides, with 2 resolved references; list the direct overrides with: c10r trace --db='…/flask-scip-python.db' --workspace='flask' 'flask::flask::flask::json::tag::JSONTag::to_json' --relation implementers
```

## Ranked dependents on Flask

Seed: `flask::flask::flask::json::provider::JSONProvider::dumps`, `--depth 2`, ranked ordering, scip-python.
Its distance-1 layer includes `JSONProvider.response`, which `DefaultJSONProvider.response` overrides.

| Rank | `main`                                  | Working tree                            |
| ---- | --------------------------------------- | --------------------------------------- |
| 1    | `json.dumps` (uses)                     | `json.dumps` (uses)                     |
| 2    | `JSONProvider.response` (uses)          | `JSONProvider.response` (uses)          |
| 3    | `JSONProvider.dump` (uses)              | `JSONProvider.dump` (uses)              |
| 4    | `Flask.create_jinja_environment` (uses) | `DefaultJSONProvider.dumps` (overrides) |
| 5    | `EnvironBuilder.json_dumps` (uses)      | `Flask.create_jinja_environment` (uses) |
| 6    |                                         | `EnvironBuilder.json_dumps` (uses)      |

The distance-1 layer gains the override of the seed and keeps the relative order of its other rows.
The detailed rows grow from 16 to 18: the override at distance 1, and `DefaultJSONProvider.response`, which uses that override, at distance 2.
The beyond-bound aggregates are unchanged.

## ripgrep

rust-analyzer 1.95.0 records 862 `overrides` edges.
The largest groups are the methods of the `Flag` trait in `rg::flags`, with 104 impls each.

Trait method: `ripgrep::grep-searcher::sink::Sink::matched`.

- `references`: 3 results, and the line `overridden by 9 methods, counting overrides of overrides, with 0 resolved references`.
- `dependents --depth 1`: 10 rows.
  Seven impl methods are reported via `overrides`: `JSONSink`, `StandardSink`, `SummarySink`, `Bytes`, `Lossy`, `UTF8`, and `KitchenSink`.
  The impls for `&mut S` and `Box<S>` are reported via `uses`, because each also calls the trait method and `uses` comes first in the tie-break order.
  The tenth row is `Core::sink_matched` via `uses`.
