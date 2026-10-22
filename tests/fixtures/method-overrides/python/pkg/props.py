class SizedBase:
    @property
    def size(self):
        return self._size

    @size.setter
    def size(self, value):
        self._size = value


class SizedChild(SizedBase):
    @property
    def size(self):
        return self._size + 1

    @size.setter
    def size(self, value):
        self._size = value + 1
