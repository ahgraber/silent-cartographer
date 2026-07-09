class Base:
    def describe(self):
        return "base"


class Mixin:
    pass


class Widget(Base):
    def render(self):
        if True:
            return "widget"
        return "unreachable"


class Gadget(Base, Mixin):
    pass
