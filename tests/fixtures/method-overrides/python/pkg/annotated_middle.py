class AnnotRoot:
    def m(self):
        return 1


class AnnotMid(AnnotRoot):
    m: object


class AnnotLeaf(AnnotMid):
    def m(self):
        return 3
