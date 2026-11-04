#!/usr/bin/env python3
"""E2E tray smoke for the Linux port (test-only tool).

Run inside `dbus-run-session` with DISPLAY set (Xvfb). Two modes:
  --with-watcher   exposes a minimal org.kde.StatusNotifierWatcher and drives
                   the full open/close/single-instance/quit lifecycle.
  --no-watcher     asserts the Phase-1 fallback: window opens at startup and
                   the stderr warning is printed.
The expected menu is not listed here: it is what `<binary> --print-menu`
prints (src/menu.rs), compared with the dbusmenu layout.
"""
import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

WATCHER_XML = """
<node>
  <interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem">
      <arg name="service" type="s" direction="in"/>
    </method>
    <method name="RegisterStatusNotifierHost">
      <arg name="service" type="s" direction="in"/>
    </method>
    <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
    <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
    <property name="ProtocolVersion" type="i" access="read"/>
    <signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
    <signal name="StatusNotifierHostRegistered"><arg type="s"/></signal>
    <signal name="StatusNotifierUnregistered"><arg type="s"/></signal>
  </interface>
</node>
"""

# The window title of Ajustes is its tab; a fresh app opens on "Geral".
SETTINGS_TITLE = "Geral"

CHECKS = []


def check(name, condition, detail=""):
    status = "PASS" if condition else "FAIL"
    CHECKS.append((name, condition))
    print(f"[{status}] {name}" + (f" — {detail}" if detail and not condition else ""), flush=True)


def windows(name="AI Monitor"):
    out = subprocess.run(
        ["xdotool", "search", "--name", name], capture_output=True, text=True
    ).stdout.split()
    return [w for w in out if w.strip()]


def printed_menu(binary):
    """The menu lines the binary itself describes (src/menu.rs)."""
    out = subprocess.run(
        [binary, "--print-menu"], capture_output=True, text=True, timeout=15
    )
    if out.returncode != 0:
        raise RuntimeError(f"--print-menu exited {out.returncode}: {out.stderr[:300]}")
    return json.loads(out.stdout)


def close_request():
    """WM_DELETE_WINDOW ClientMessage (compositor close), needs python3-xlib."""
    # No fallback: without a WM, xdotool cannot send a close request, and a
    # silent no-op would turn this check into a false failure or a false pass.
    from Xlib import X, display, protocol

    d = display.Display()
    wm_protocols = d.intern_atom("WM_PROTOCOLS")
    wm_delete = d.intern_atom("WM_DELETE_WINDOW")
    for wid in windows():
        w = d.create_resource_object("window", int(wid))
        ev = protocol.event.ClientMessage(
            window=w,
            client_type=wm_protocols,
            data=(32, [wm_delete, X.CurrentTime, 0, 0, 0]),
        )
        w.send_event(ev)
    d.sync()
    d.close()


def rss_kb(pid):
    try:
        for line in open(f"/proc/{pid}/status"):
            if line.startswith("VmRSS"):
                return int(line.split()[1])
    except FileNotFoundError:
        return -1
    return -1


class FakeWatcher:
    def __init__(self, conn):
        self.conn = conn
        self.items = []
        self.registered = GLib.Cond()
        node = Gio.DBusNodeInfo.new_for_xml(WATCHER_XML)
        iface = node.interfaces[0]
        conn.register_object(
            "/StatusNotifierWatcher", iface, self.on_method, self.on_get, None
        )
        Gio.bus_own_name_on_connection(
            conn,
            "org.kde.StatusNotifierWatcher",
            Gio.BusNameOwnerFlags.NONE,
            None,
            None,
        )
        # Also claim the legacy kde alias some clients probe.
        Gio.bus_own_name_on_connection(
            conn,
            "org.freedesktop.StatusNotifierWatcher",
            Gio.BusNameOwnerFlags.NONE,
            None,
            None,
        )

    def on_method(self, conn, sender, path, iface, method, params, invocation):
        if method == "RegisterStatusNotifierItem":
            service = params.unpack()[0]
            self.items.append(service)
            self.conn.emit_signal(
                None,
                path,
                iface,
                "StatusNotifierItemRegistered",
                GLib.Variant("(s)", (service,)),
            )
        elif method == "RegisterStatusNotifierHost":
            self.conn.emit_signal(
                None,
                path,
                iface,
                "StatusNotifierHostRegistered",
                GLib.Variant("(s)", (params.unpack()[0],)),
            )
        invocation.return_value(None)

    def on_get(self, conn, sender, path, iface, prop):
        if prop == "IsStatusNotifierHostRegistered":
            return GLib.Variant("b", True)
        if prop == "RegisteredStatusNotifierItems":
            return GLib.Variant("as", self.items)
        if prop == "ProtocolVersion":
            return GLib.Variant("i", 0)
        return None


def pump(duration):
    """Iterate the GLib main context so our D-Bus objects get dispatched."""
    ctx = GLib.MainContext.default()
    deadline = time.monotonic() + duration
    while True:
        while ctx.pending():
            ctx.iteration(False)
        if time.monotonic() >= deadline:
            return
        time.sleep(0.02)


def until(predicate, timeout, what):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        pump(0.2)
        if predicate():
            return True
    print(f"TIMEOUT waiting for: {what}", flush=True)
    return False


def dbusmenu(conn, item):
    """Return (menu_path, proxy) for the registered SNI's dbusmenu."""
    sni = Gio.DBusProxy.new_sync(
        conn,
        Gio.DBusProxyFlags.NONE,
        None,
        item,
        "/StatusNotifierItem",
        "org.freedesktop.DBus.Properties",
        None,
    )
    try:
        path = (
            sni.call_sync(
                "Get",
                GLib.Variant("(ss)", ("org.kde.StatusNotifierItem", "Menu")),
                Gio.DBusCallFlags.NONE,
                5000,
                None,
            )
            .unpack()[0]
            .unpack()
        )
    except Exception:
        path = "/MenuBar"
    proxy = Gio.DBusProxy.new_sync(
        conn,
        Gio.DBusProxyFlags.NONE,
        None,
        item,
        path,
        "com.canonical.dbusmenu",
        None,
    )
    # Prove the menu actually answers before caching it.
    proxy.call_sync(
        "GetLayout",
        GLib.Variant("(iias)", (0, -1, [])),
        Gio.DBusCallFlags.NONE,
        5000,
        None,
    )
    return path, proxy


def layout(proxy):
    """[(id, label, enabled, visible, separator)] of the top-level items.

    dbusmenu omits default properties: enabled and visible default to true,
    an empty label is absent. `__` is how the app escapes a literal `_`.
    """
    rev, root = proxy.call_sync(
        "GetLayout",
        GLib.Variant("(iias)", (0, -1, [])),
        Gio.DBusCallFlags.NONE,
        5000,
        None,
    ).unpack()
    items = []
    for child in root[2]:
        props = child[1]
        items.append(
            (
                child[0],
                (props.get("label") or "").replace("__", "_"),
                props.get("enabled", True),
                props.get("visible", True),
                props.get("type") == "separator",
            )
        )
    return items


def menu_mismatches(expected, items):
    """Differences between --print-menu and the dbusmenu layout, if any."""
    if len(items) != len(expected):
        return [f"{len(items)} items, --print-menu has {len(expected)}"]
    wrong = []
    for i, (want, (_, label, enabled, visible, separator)) in enumerate(
        zip(expected, items)
    ):
        if separator != want["separator"]:
            wrong.append(f"{i}: separator {separator}, expected {want['separator']}")
            continue
        if separator:
            continue
        if enabled != want["enabled"]:
            wrong.append(f"{i} {label!r}: enabled {enabled}")
        if want["dynamic"]:
            # Data-dependent text. A line --print-menu shows stays shown; only
            # the one it hides (the alert band, shown with alerts) is free.
            if want["visible"] and not visible:
                wrong.append(f"{i} {label!r}: hidden, --print-menu shows it")
            if visible and not label:
                wrong.append(f"{i}: visible dynamic line without a label")
        elif (label, visible) != (want["label"], want["visible"]):
            wrong.append(f"{i}: {label!r} visible={visible}, expected {want['label']!r}")
    return wrong


def click(proxy, item_id):
    proxy.call_sync(
        "Event",
        GLib.Variant("(isvu)", (item_id, "clicked", GLib.Variant("s", ""), 0)),
        Gio.DBusCallFlags.NONE,
        5000,
        None,
    )


def sni_property(conn, item, name):
    sni = Gio.DBusProxy.new_sync(
        conn,
        Gio.DBusProxyFlags.NONE,
        None,
        item,
        "/StatusNotifierItem",
        "org.freedesktop.DBus.Properties",
        None,
    )
    return sni.call_sync(
        "Get",
        GLib.Variant("(ss)", ("org.kde.StatusNotifierItem", name)),
        Gio.DBusCallFlags.NONE,
        5000,
        None,
    ).unpack()[0]


def icon_pixmap(conn, item):
    sni = Gio.DBusProxy.new_sync(
        conn,
        Gio.DBusProxyFlags.NONE,
        None,
        item,
        "/StatusNotifierItem",
        "org.freedesktop.DBus.Properties",
        None,
    )
    result = sni.call_sync(
        "Get",
        GLib.Variant("(ss)", ("org.kde.StatusNotifierItem", "IconPixmap")),
        Gio.DBusCallFlags.NONE,
        5000,
        None,
    )
    return result.unpack()[0]


def scenario_with_watcher(binary):
    conn = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    watcher = FakeWatcher(conn)
    pump(0.5)  # let the well-known names be owned before the client connects
    # A session's private runtime dir (logind gives one, 0700): without a
    # private folder the app writes no SVG and keeps the pixmap only.
    runtime = tempfile.mkdtemp(prefix="ai-monitor-smoke-")
    env = dict(os.environ, XDG_RUNTIME_DIR=runtime)
    proc = subprocess.Popen(
        [binary], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, env=env
    )
    try:
        check(
            "SNI item registered",
            until(lambda: len(watcher.items) > 0, 15, "registration"),
            "no RegisterStatusNotifierItem call",
        )
        item = watcher.items[0] if watcher.items else None
        if not item:
            return
        try:
            px = icon_pixmap(conn, item)
            icons = px.unpack() if isinstance(px, GLib.Variant) else px
        except Exception as e:
            icons = []
            print(f"IconPixmap read error: {e}", flush=True)
        check(
            "IconPixmap non-empty",
            len(icons) > 0 and len(icons[0][2]) > 0,
            f"got {icons!r}",
        )
        # The symbolic icon: a name the host finds under IconThemePath.
        try:
            name = sni_property(conn, item, "IconName")
            path = sni_property(conn, item, "IconThemePath")
        except Exception as e:
            name, path = "", ""
            print(f"IconName read error: {e}", flush=True)
        svg = os.path.join(path, "hicolor", "scalable", "status", f"{name}.svg")
        check(
            "IconName is a written symbolic SVG",
            name.endswith("-symbolic")
            and path.startswith(runtime + os.sep)
            and os.path.isfile(svg),
            f"name={name!r} path={path!r}",
        )

        expected = printed_menu(binary)
        check(
            "--print-menu lists the menu",
            len(expected) > 0 and any(e["action"] == "quit" for e in expected),
            f"got {expected!r}",
        )
        menu_path, menu = None, None
        ok = until(
            lambda: _try_menu(conn, item), 10, "dbusmenu available"
        )
        state = _try_menu(conn, item)
        check("dbusmenu available", state is not None)
        by_action = {}
        if state:
            menu_path, menu = state
            items = layout(menu)
            wrong = menu_mismatches(expected, items)
            check(
                "dbusmenu matches --print-menu",
                not wrong,
                f"{wrong}; got {[i[1:] for i in items]}",
            )
            # Click by action (the first line with it): the handoffs line is
            # the dynamic "sessions" one; "review" is the proposals line.
            for want, got in zip(expected, items):
                if want["action"] and want["action"] not in by_action:
                    by_action[want["action"]] = got[0]
            # The static "review" line is "Revisar propostas…".
            static_review = [
                got[0]
                for want, got in zip(expected, items)
                if want["action"] == "review" and not want["dynamic"]
            ]
            if static_review:
                by_action["review-static"] = static_review[0]

        rss = []
        check("no window at startup", not windows(), f"windows: {windows()}")
        rss.append(("initial", rss_kb(proc.pid)))

        def open_and_close(action, cycles=1, settings=False):
            """Open by a menu action, then close as the compositor would."""
            for _ in range(cycles):
                click(menu, by_action[action])
                ok = until(lambda: len(windows()) == 1, 15, f"open via {action}")
                check(f"{action} opens one window", ok, f"windows: {windows()}")
                if settings:
                    check(
                        f"{action} also opens Ajustes",
                        until(lambda: len(windows(SETTINGS_TITLE)) == 1, 15, "Ajustes"),
                        f"windows: {windows(SETTINGS_TITLE)}",
                    )
                rss.append(("open", rss_kb(proc.pid)))
                close_request()
                ok = until(
                    lambda: not windows()
                    and not windows(SETTINGS_TITLE)
                    and proc.poll() is None,
                    15,
                    f"close after {action}",
                )
                check(f"compositor close after {action}: no window, process alive", ok)
                rss.append(("close", rss_kb(proc.pid)))

        open_and_close("panel", cycles=3)
        open_and_close("health")
        open_and_close("review-static")
        open_and_close("sessions")
        open_and_close("review")
        open_and_close("settings", settings=True)
        # Ajustes over a window that is already open.
        click(menu, by_action["panel"])
        check(
            "window open before Ajustes",
            until(lambda: len(windows()) == 1, 15, "open for Ajustes"),
        )
        click(menu, by_action["settings"])
        check(
            "Ajustes opens over the open window",
            until(lambda: len(windows(SETTINGS_TITLE)) == 1 and len(windows()) == 1, 15, "Ajustes"),
            f"windows: {windows()} / {windows(SETTINGS_TITLE)}",
        )
        close_request()
        check(
            "closed before the second instance",
            until(lambda: not windows() and not windows(SETTINGS_TITLE), 15, "close"),
            f"windows: {windows()} / {windows(SETTINGS_TITLE)}",
        )

        # Second instance: forwards Show and exits 0; first reopens.
        second = subprocess.run(
            [binary], capture_output=True, text=True, timeout=15, env=env
        )
        check("second instance exits 0", second.returncode == 0, second.stderr[:300])
        check(
            "second instance reopened the dashboard",
            until(lambda: len(windows()) == 1, 15, "Show forwarded"),
            f"windows: {windows()}",
        )
        rss.append(("reopen", rss_kb(proc.pid)))

        click(menu, by_action["quit"])
        check(
            "Sair quits cleanly",
            until(lambda: proc.poll() == 0, 15, "process exit"),
            f"poll={proc.poll()}",
        )
        print("RSS per stage (KiB):", rss, flush=True)
    finally:
        if proc.poll() is None:
            proc.kill()
        stderr = proc.stderr.read() if proc.stderr else ""
        if stderr.strip():
            print("app stderr:", stderr.strip()[:500], flush=True)
        shutil.rmtree(runtime, ignore_errors=True)


_menu_cache = {}


def _try_menu(conn, item):
    if item in _menu_cache:
        return _menu_cache[item]
    try:
        result = dbusmenu(conn, item)
        _menu_cache[item] = result
        return result
    except Exception:
        return None


def scenario_no_watcher(binary):
    proc = subprocess.Popen(
        [binary], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True
    )
    try:
        check(
            "fallback: window opens at startup",
            until(lambda: len(windows()) == 1, 15, "window"),
            f"windows: {windows()}",
        )
        time.sleep(1)
        alive = proc.poll() is None
        check("fallback: process alive", alive, f"poll={proc.poll()}")
        close_request()
        check(
            "fallback: close request exits 0",
            until(lambda: proc.poll() == 0, 10, "exit after window close"),
            f"poll={proc.poll()}",
        )
    finally:
        if proc.poll() is None:
            proc.kill()
        stderr = proc.stderr.read() if proc.stderr else ""
        check(
            "fallback: warning on stderr",
            "StatusNotifierWatcher" in stderr or "bandeja" in stderr,
            stderr.strip()[:300],
        )


def main():
    p = argparse.ArgumentParser()
    p.add_argument("binary")
    p.add_argument("--with-watcher", action="store_true")
    p.add_argument("--no-watcher", action="store_true")
    args = p.parse_args()
    if args.with_watcher:
        scenario_with_watcher(args.binary)
    elif args.no_watcher:
        scenario_no_watcher(args.binary)
    else:
        p.error("choose --with-watcher or --no-watcher")
    failed = [name for name, ok in CHECKS if not ok]
    print(f"{len(CHECKS) - len(failed)} passed, {len(failed)} failed", flush=True)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
