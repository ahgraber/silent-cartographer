class A3:
    def m(self):
        return 1


class B3(A3):
    def m(self):
        return 2


class C3(B3):
    def m(self):
        return 3


class A2:
    def m(self):
        return 1


class B2(A2):
    pass


class C2(B2):
    def m(self):
        return 3
