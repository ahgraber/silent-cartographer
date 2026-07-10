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

    @property
    def size(self):
        return self._size

    @size.setter
    def size(self, value):
        self._size = value


class Gadget(Base, Mixin):
    pass
