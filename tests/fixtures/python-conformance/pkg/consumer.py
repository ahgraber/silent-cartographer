from pkg.shapes import Widget
from pkg import shapes
from pkg.shapes import Widget as W

MODULE_NAME = __name__


def build():
    w = Widget()
    return w.render()


def build_aliased():
    w = W()
    return w.render()
