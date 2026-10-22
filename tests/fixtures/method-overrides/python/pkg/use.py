from pkg.annotated_middle import AnnotLeaf
from pkg.assigned_middle import AssignLeaf
from pkg.chain import A2, A3, B3, C2, C3
from pkg.decorated import DecoratedBase, DecoratedChild
from pkg.diamond import DiaB, DiaC, DiaP, DiaQ
from pkg.imported_base import Origin
from pkg.imported_child import Derived
from pkg.multi import E, X, Y
from pkg.plain import PlainBase, PlainChild
from pkg.private import PrivBase, PrivChild
from pkg.props import SizedBase, SizedChild
from pkg.shadow import MethodChild
from pkg.stdlib import Lookup, Worker
from pkg.unrelated import Loner, Stranger


def use_plain(base: PlainBase, child: PlainChild):
    return base.speak() + child.speak()


def use_chain(a: A2, c: C2, a3: A3, b3: B3, c3: C3):
    return a.m() + c.m() + a3.m() + b3.m() + c3.m()


def use_multi(x: X, y: Y, e: E):
    return x.m() + y.m() + e.m()


def use_imported(origin: Origin, derived: Derived):
    return origin.m() + derived.m()


def use_decorated(base: DecoratedBase, child: DecoratedChild):
    return base.prop + child.prop + base.make() + child.make() + base.util() + child.util()


def use_shadow(child: MethodChild):
    return child.x()


def use_middles(assigned: AssignLeaf, annotated: AnnotLeaf):
    return assigned.m() + annotated.m()


def use_private(base: PrivBase, child: PrivChild):
    return base.__eq__(child) and child.__eq__(base) and base.call_private() + child.call_private()


def use_props(base: SizedBase, child: SizedChild):
    base.size = 1
    child.size = 2
    return base.size + child.size


def use_diamond(p: DiaP, q: DiaQ, b: DiaB, c: DiaC):
    return p.m() + q.m() + b.m() + c.m()


def use_unrelated(loner: Loner, stranger: Stranger):
    return loner.run() + stranger.run()


def use_stdlib(worker: Worker, lookup: Lookup):
    worker.run()
    return lookup.__getitem__("k")
