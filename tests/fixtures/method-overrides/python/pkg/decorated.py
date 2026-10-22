class DecoratedBase:
    @property
    def prop(self):
        return 1

    @classmethod
    def make(cls):
        return cls()

    @staticmethod
    def util():
        return 1


class DecoratedChild(DecoratedBase):
    @property
    def prop(self):
        return 2

    @classmethod
    def make(cls):
        return cls()

    @staticmethod
    def util():
        return 2
