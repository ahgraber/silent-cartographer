class X:
    def m(self):
        return "x"


class Y:
    def m(self):
        return "y"


class E(X, Y):
    def m(self):
        return "e"
