class DiaP:
    def m(self):
        return "p"


class DiaQ:
    def m(self):
        return "q"


class DiaB(DiaP, DiaQ):
    pass


class DiaC(DiaB):
    def m(self):
        return "c"
