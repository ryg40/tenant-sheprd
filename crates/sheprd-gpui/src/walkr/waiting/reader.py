# The read-only reader that Sheprd sends to an SSH host on standard input,
# run there as `python3 -I - MODE`. It writes no file and keeps nothing.
#
# `list` prints one JSON line for each saved Compare session that it reads,
# then an `end` line. `page ID` prints the address of the private Compare
# page of the Walkr server of this home, with `#ID`, or why there is none.
# The token of the server stays on the host: only the page address leaves.
#
# The home is the trust anchor: its real path opens as it is, a symlink
# included. Each folder below it opens from the parent handle with
# O_NOFOLLOW, and each record with O_NOFOLLOW and O_NONBLOCK; the checks run
# on the opened handle.
#
# SIGALRM ends the program at its deadline wherever it blocks, with the
# exit status TIMED_OUT, which Sheprd reads as "not in time".
import errno
import hashlib
import http.client
import json
import os
import re
import signal
import stat
import sys
import time

SECONDS = 15
DEADLINE = time.monotonic() + SECONDS
TIMED_OUT = 4
PROTOCOL = 9
ENTRY_LIMIT = 5000
RECORD_LIMIT = 16 * 1024 * 1024
PREFIX = 8 * 1024
REPO_PREFIX = 64 * 1024
SERVER_LIMIT = 64 * 1024
ANSWER_LIMIT = 16 * 1024
TITLE = 200
SHORT = 64
REPO = 100
SESSIONS = (".local", ".walkr", "compare", "sessions")
SESSION_ID = re.compile(r"\A[0-9]{8}T[0-9]{6}Z-[0-9a-f]{12}\Z")
PAGE_PATH = re.compile(r"\A/_walkr/compare/g/[0-9a-f]{1,256}\Z")
DIR_FLAGS = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
FILE_FLAGS = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
WANTED = ("schema", "version", "sessionId", "createdAt", "status", "title", "repo")


class Absent(Exception):
    """A folder is missing, or is not a folder of its own (a symlink)."""


def out_of_time(*_):
    sys.stderr.write("walkr reader: out of time\n")
    sys.stderr.flush()
    # A hard exit: the deadline holds inside any blocked call or handler.
    os._exit(TIMED_OUT)


def late():
    if time.monotonic() > DEADLINE:
        out_of_time()


def emit(value):
    sys.stdout.write(json.dumps(value, ensure_ascii=True, separators=(",", ":")) + "\n")


def open_dir(name, parent):
    try:
        return os.open(name, DIR_FLAGS, dir_fd=parent)
    except (FileNotFoundError, NotADirectoryError):
        raise Absent()
    except OSError as error:
        # ELOOP: the name is a symlink, which counts as no folder.
        if error.errno == errno.ELOOP:
            raise Absent()
        raise


def walk(parts):
    # The real path of the home, so a home that is a symlink stays readable.
    home = os.path.realpath(os.path.expanduser("~"))
    folder = os.open(home, DIR_FLAGS & ~os.O_NOFOLLOW)
    for part in parts:
        try:
            child = open_dir(part, folder)
        finally:
            os.close(folder)
        folder = child
    return folder


def read_file(name, folder, limit, prefix):
    """At most `prefix` bytes of a regular file of at most `limit` bytes."""
    fd = os.open(name, FILE_FLAGS, dir_fd=folder)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
            return None
        chunks = []
        left = prefix
        while left > 0:
            late()
            try:
                chunk = os.read(fd, min(left, 65536))
            except BlockingIOError:
                return None
            if not chunk:
                break
            chunks.append(chunk)
            left -= len(chunk)
        return b"".join(chunks)
    finally:
        os.close(fd)


def skip(text, at):
    while at < len(text) and text[at] in " \t\r\n":
        at += 1
    return at


def fields(data):
    """The wanted top-level keys of a JSON object cut at any byte: the
    values before the cut count, the first copy of a key wins."""
    text = data.decode("utf-8", "ignore")
    decoder = json.JSONDecoder()
    found = {}
    at = skip(text, 0)
    if not text.startswith("{", at):
        return found
    at += 1
    while len(found) < len(WANTED):
        at = skip(text, at)
        try:
            key, at = decoder.raw_decode(text, at)
            at = skip(text, at)
            if not isinstance(key, str) or not text.startswith(":", at):
                break
            value, at = decoder.raw_decode(text, skip(text, at + 1))
        except (ValueError, RecursionError):
            break
        if key in WANTED and key not in found:
            found[key] = value
        at = skip(text, at)
        if not text.startswith(",", at):
            break
        at += 1
    return found


class Unreadable(Exception):
    """A text field that is not valid Unicode: its record is skipped."""


def short(value, limit):
    if not isinstance(value, str):
        return None
    try:
        # A lone surrogate from a `\ud83d` escape has no UTF-8 form.
        value.encode("utf-8")
    except UnicodeEncodeError:
        raise Unreadable()
    return value[:limit]


def repo_name(value):
    name = value.get("name") if isinstance(value, dict) else None
    return short(name, REPO)


def session(root, name):
    try:
        folder = open_dir(name, root)
    except (Absent, OSError):
        return None
    try:
        data = read_file("session.json", folder, RECORD_LIMIT, PREFIX)
        if data is None:
            return None
        found = fields(data)
        if found.get("status") == "open" and "repo" not in found:
            data = read_file("session.json", folder, RECORD_LIMIT, REPO_PREFIX)
            found = fields(data) if data is not None else found
    except OSError:
        return None
    finally:
        os.close(folder)
    if (
        found.get("schema") != "walkr-compare-session"
        or found.get("version") != 1
        or found.get("sessionId") != name
    ):
        return None
    try:
        return {
            "sessionId": name,
            "status": short(found.get("status"), SHORT),
            "createdAt": short(found.get("createdAt"), SHORT),
            "title": short(found.get("title"), TITLE),
            "repo": repo_name(found.get("repo")),
        }
    except Unreadable:
        return None


def list_sessions():
    try:
        root = walk(SESSIONS)
    except Absent:
        emit({"end": "absent"})
        return
    try:
        with os.scandir(root) as entries:
            for seen, entry in enumerate(entries):
                if seen >= ENTRY_LIMIT:
                    break
                late()
                name = entry.name
                if not SESSION_ID.match(name) or not entry.is_dir(follow_symlinks=False):
                    continue
                found = session(root, name)
                if found is not None:
                    emit({"session": found})
    finally:
        os.close(root)
    emit({"end": "read"})


def whole(value):
    # JSON `true` is an int in Python, and `5.0` equals `5`: neither is a pid.
    return isinstance(value, int) and not isinstance(value, bool)


def ask(port, path, token=None):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=5)
    headers = {"x-human-review-token": token} if token is not None else {}
    try:
        connection.request("GET", path, headers=headers)
        answer = connection.getresponse()
        body = answer.read(ANSWER_LIMIT + 1)
        if answer.status != 200 or len(body) > ANSWER_LIMIT:
            return None
        value = json.loads(body.decode("utf-8"))
        return value if isinstance(value, dict) else None
    finally:
        connection.close()


def page(session_id):
    if not SESSION_ID.match(session_id):
        emit({"none": "session"})
        return
    try:
        folder = walk((".human-review",))
    except Absent:
        emit({"none": "record"})
        return
    try:
        data = read_file("server.json", folder, SERVER_LIMIT, SERVER_LIMIT)
    except OSError:
        # Missing, a symlink, or unreadable: no record to trust.
        data = None
    finally:
        os.close(folder)
    try:
        record = json.loads(data.decode("utf-8")) if data is not None else None
    except ValueError:
        record = None
    port = record.get("port") if isinstance(record, dict) else None
    token = record.get("token") if isinstance(record, dict) else None
    pid = record.get("pid") if isinstance(record, dict) else None
    if (
        not whole(port)
        or not 0 < port < 65536
        or not whole(pid)
        or not isinstance(token, str)
        or not 0 < len(token) <= 1024
        or record.get("protocol") != PROTOCOL
    ):
        emit({"none": "record"})
        return
    # Walkr's server computes its root from the home as given, not its real path.
    root = os.path.join(os.path.expanduser("~"), ".local", ".walkr", "compare")
    try:
        # The token goes only to the Walkr server of the record: its pid and
        # protocol, as Walkr itself checks before it reuses a server.
        health = ask(port, "/health")
        if (
            health is None
            or health.get("ok") is not True
            or health.get("protocol") != PROTOCOL
            or not whole(health.get("pid"))
            or health.get("pid") != pid
        ):
            emit({"none": "server"})
            return
        capabilities = ask(port, "/_walkr/compare/api/capabilities", token)
        if (
            capabilities is None
            or capabilities.get("compare") != 1
            or capabilities.get("rootId") != hashlib.sha256(root.encode()).hexdigest()
        ):
            emit({"none": "server"})
            return
        answer = ask(port, "/_walkr/compare/api/page", token)
    except (OSError, ValueError, http.client.HTTPException):
        emit({"none": "stopped"})
        return
    path = answer.get("path") if answer is not None else None
    if not isinstance(path, str) or not PAGE_PATH.match(path):
        emit({"none": "server"})
        return
    emit({"page": "http://127.0.0.1:%d%s#%s" % (port, path, session_id)})


def main():
    signal.signal(signal.SIGALRM, out_of_time)
    signal.alarm(SECONDS)
    mode = sys.argv[1:]
    if mode == ["list"]:
        list_sessions()
    elif len(mode) == 2 and mode[0] == "page":
        page(mode[1])
    else:
        sys.exit(2)


main()
