import json

from pkg.client import Client


def use_annotated(client: Client):
    return client.fetch()


def drain_it(client):
    return client.drain()


def run(callback):
    return callback()


def run_local():
    helper = make_helper()
    return helper()


def make_helper():
    return run


shared = json.loads("[]")
shared.append(1)


def wrap(text):
    return json.loads(text)


def nested():
    return wrap(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    ).strip()
