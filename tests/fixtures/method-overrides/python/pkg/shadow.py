class AttrBase:
    x = 1


class MethodChild(AttrBase):
    def x(self):
        return 2
