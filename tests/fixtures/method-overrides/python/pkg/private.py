class PrivBase:
    def __m(self):
        return 1

    def __eq__(self, other):
        return True

    def call_private(self):
        return self.__m()


class PrivChild(PrivBase):
    def __m(self):
        return 2

    def __eq__(self, other):
        return False

    def call_private(self):
        return self.__m()
