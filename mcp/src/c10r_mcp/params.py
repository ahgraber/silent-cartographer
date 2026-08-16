"""The closed value sets the tool schemas carry.

Typing these as enumerations puts the valid values in the schema where a model
reads them before choosing, which prevents the error rather than reporting it.
Constraints spanning more than one parameter — a depth or ordering option only
meaningful with one relation — are not re-checked here; `c10r` owns them and
rejects them, and a second copy would drift.
"""

from __future__ import annotations

from enum import StrEnum


class Detail(StrEnum):
    """How much of a symbol to project onto a result."""

    LOCATION = "location"
    """The definition file and position."""

    SIGNATURE = "signature"
    """The signature, without the body."""

    INTERFACE = "interface"
    """The signature together with the symbol's own documentation."""

    BODY = "body"
    """The full source body."""


class Relation(StrEnum):
    """The relation a trace follows from its subject."""

    CONTAINERS = "containers"
    """The declaration that directly encloses the subject."""

    CONTAINS = "contains"
    """The symbols the subject directly contains."""

    REFERENCES = "references"
    """The sites that reference the subject."""

    DEPENDENTS = "dependents"
    """Everything depending on the subject, directly and transitively."""

    IMPORTERS = "importers"
    """The modules that import the subject."""

    IMPLEMENTERS = "implementers"
    """The types declaring the subject as a supertype."""

    TESTS = "tests"
    """The reference sites whose enclosing declaration is test code."""


class Language(StrEnum):
    """The language backend a build indexes with."""

    RUST = "rust"
    PYTHON = "python"


class Order(StrEnum):
    """What breaks ties within a distance layer of a dependents answer."""

    RANKED = "ranked"
    """Structural importance first — guidance for reading order, not fact."""

    UNRANKED = "unranked"
    """Distance, then dependency kind, then identity; free of any ranking model."""
