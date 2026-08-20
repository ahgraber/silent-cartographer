# Code Graph — Delta: internal-efficiency

## ADDED Requirements

### Requirement: A build over unchanged inputs does no work

Before analyzing a workspace, the system SHALL determine whether the stored index already describes that workspace: the same workspace identity and root, and the workspace's current sources, analyzer, and declared environment.
When it does, the system SHALL analyze nothing, leave the store unchanged, report the index as already current, and succeed.
When any of those inputs differs, or no compatible store exists, the system SHALL build.
The system SHALL offer an explicit way to build even when the stored index is already current.

Serves: rebuild-costs-what-changed

#### Scenario: Rebuilding an unchanged workspace analyzes nothing

- **GIVEN** a workspace whose index was just built
- **WHEN** the workspace is built again with no source, analyzer, or environment change
- **THEN** no analysis runs, the store is unchanged, and the index is reported as already current

#### Scenario: An edited source rebuilds

- **GIVEN** a workspace whose index was just built
- **WHEN** one source file is edited and the workspace is built again
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A changed analyzer rebuilds

- **GIVEN** a workspace whose index was built by one analyzer version
- **WHEN** the workspace is built again with a different analyzer version
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A changed environment rebuilds

- **GIVEN** a workspace whose index records a declared interpreter environment
- **WHEN** the workspace is built again against a different environment
- **THEN** the workspace is analyzed and the store is rebuilt

#### Scenario: A store recorded for another workspace builds

- **GIVEN** a store whose recorded workspace identity or root differs from the one being built
- **WHEN** the workspace is built over that store
- **THEN** the workspace is analyzed, the handoff is disclosed, and the store records the workspace it now describes

#### Scenario: An absent index builds

- **GIVEN** a workspace with no index
- **WHEN** the workspace is built
- **THEN** the workspace is analyzed and the store is written

#### Scenario: A store built under an unrecognized schema version builds

- **GIVEN** a workspace whose store the system created under a schema version it does not recognize
- **WHEN** the workspace is built
- **THEN** the workspace is analyzed and the store is replaced

#### Scenario: An explicit rebuild ignores currency

- **GIVEN** a workspace whose index is already current
- **WHEN** the workspace is built with the explicit rebuild option
- **THEN** the workspace is analyzed and the store is rebuilt

### Requirement: Deriving the graph costs one pass over each document

The system SHALL parse each document it analyzes at most once per build, and the parse work a build performs SHALL NOT grow with the number of symbols or definitions its documents declare.

Serves: rebuild-costs-what-changed

#### Scenario: A symbol-dense document costs no more than a sparse one of the same size

- **GIVEN** two documents of equal size, one declaring many symbols and one declaring few
- **WHEN** each is built
- **THEN** the parse work performed for each is the same

#### Scenario: Adding declarations to a document does not add parse work

- **GIVEN** a workspace built once, and the same workspace after further declarations are added to one document
- **WHEN** each is built
- **THEN** the parse work performed for that document is unchanged

#### Scenario: Parse work follows the documents analyzed

- **GIVEN** a workspace of several documents
- **WHEN** the workspace is built
- **THEN** the parse work performed is proportional to those documents

## MODIFIED Requirements

### Requirement: Reference occurrences carry enclosing-declaration attribution

> Previously: the requirement constrained only where an occurrence is attributed and said nothing about the work performed to resolve it, so an implementation that examined every persisted definition for each occurrence satisfied it. It was also silent on a location holding more than one persisted definition, which the implementation resolved to an arbitrary one.

The system SHALL attribute every aligned reference occurrence to the nearest enclosing persisted declaration in the fresh syntax tree, and SHALL attribute an occurrence enclosed by no narrower declaration to its module — never to a fabricated declaration.
Where more than one persisted definition shares a location, the system SHALL NOT attribute an occurrence to any of them, and SHALL attribute it as though no persisted declaration enclosed it.
The work performed to resolve one occurrence's attribution SHALL be bounded by that occurrence's enclosing-declaration chain, and SHALL NOT grow with the number of definitions the build persists.

Serves: rebuild-costs-what-changed

#### Scenario: Reference inside a method attributes to the method

- **GIVEN** an aligned reference occurrence located inside a method body
- **WHEN** the attribution is recorded
- **THEN** the occurrence is attributed to that method

#### Scenario: Reference inside a closure attributes to the declaring function

- **GIVEN** an aligned reference occurrence inside a closure defined within a function
- **WHEN** the attribution is recorded
- **THEN** the occurrence is attributed to the enclosing function, since the closure is not a persisted symbol

#### Scenario: Module-level reference attributes to the module

- **GIVEN** an aligned reference occurrence at module scope, enclosed by no narrower declaration
- **WHEN** the attribution is recorded
- **THEN** the occurrence is attributed to the module

#### Scenario: An ambiguous enclosing location attributes to no declaration

- **GIVEN** an aligned reference occurrence whose nearest enclosing declaration's location holds the definitions of two distinct persisted symbols
- **WHEN** the attribution is recorded
- **THEN** the occurrence is attributed to its module rather than to either symbol

#### Scenario: A larger workspace does not cost more per attribution

- **GIVEN** two workspaces with the same enclosing-declaration chains, one persisting many times as many definitions as the other
- **WHEN** references are attributed in each
- **THEN** the work performed per attribution is the same in both
