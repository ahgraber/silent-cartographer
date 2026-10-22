class PlainBase:
    def speak(self):
        return "base"


class PlainChild(PlainBase):
    def speak(self):
        return "child"
