class AssignRoot:
    def m(self):
        return 1


class AssignMid(AssignRoot):
    m = None


class AssignLeaf(AssignMid):
    def m(self):
        return 3
