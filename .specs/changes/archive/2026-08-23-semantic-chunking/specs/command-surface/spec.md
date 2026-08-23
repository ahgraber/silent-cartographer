# Delta for Command Surface

## ADDED Requirements

### Requirement: Chunk parameters on build

The `build` command SHALL accept the two chunk parameters — the chunk size and the overlap between adjacent chunks — as canonical flags of the closed option vocabulary, and SHALL apply a recommended default to each when it is not supplied, so that an operator can vary the retrieval tradeoff without rebuilding the tool.

The chunk size SHALL bound the whole text a chunk is embedded from, its passage header included, so that the size an operator supplies is the size the embedding model receives.

A supplied value SHALL be validated at the boundary: a chunk size that cannot admit content beside a passage's header, or an overlap not smaller than the chunk size, SHALL be rejected as a usage error rather than silently clamped.

Serves: discriminating-long-symbol

#### Scenario: Defaults apply when unsupplied

- **GIVEN** a workspace to index
- **WHEN** `build` is invoked with neither parameter
- **THEN** the build completes and the store records the recommended defaults as the parameters in effect

#### Scenario: Supplied values take effect

- **GIVEN** a workspace to index
- **WHEN** `build` is invoked with an explicit chunk size and overlap
- **THEN** the build completes and the store records the supplied values as the parameters in effect

#### Scenario: No chunk exceeds the supplied size

- **GIVEN** a workspace holding a symbol whose content far exceeds the chunk size
- **WHEN** `build` is invoked with an explicit chunk size
- **THEN** every chunk the build embeds is within that size, header included

#### Scenario: An overlap that is not smaller than the chunk size is refused

- **GIVEN** a workspace to index
- **WHEN** `build` is invoked with an overlap equal to or greater than the chunk size
- **THEN** the invocation is rejected as a usage error and no build is performed

#### Scenario: A chunk size leaving no room for content is refused

- **GIVEN** a workspace to index
- **WHEN** `build` is invoked with a chunk size too small to admit content beside a passage's header
- **THEN** the invocation is rejected as a usage error and no build is performed
