import threading


class Worker(threading.Thread):
    def run(self):
        return None


class Lookup(dict):
    def __getitem__(self, key):
        return 0
