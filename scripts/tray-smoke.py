#!/usr/bin/env python3
"""E2E tray smoke for the Linux port (test-only tool).

Run inside `dbus-run-session` with DISPLAY set (Xvfb). Two modes:
  --with-watcher   exposes a minimal org.kde.StatusNotifierWatcher and drives
                   the full open/hide/single-instance/quit lifecycle.
  --no-watcher     asserts the Phase-1 fallback: window opens at startup and
                   the stderr warning is printed.
"""
import argparse
import os
import subprocess
import sys
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

EXPECTED_HEAD = [
    "AI Monitor",
    "ai-memory · métricas indisponíveis",
    "Docker (contêineres) · métricas indisponíveis",
]
EXPECTED_TAIL = [
    "CPU: 100% = 1 núcleo · M/G = MiB/GiB",
    "Docker = contêineres ativos, incluindo ai-memory",
    "---",
    "Abrir painel",
    "Saúde e sessões",
    "Revisar propostas e handoffs",
    "Administração avançada",
    "Ocultar painel",
    "Sair do AI Monitor",
]

CHECKS = []


def check(name, condition, detail=""):
    status = "PASS" if condition else "FAIL"
    CHECKS.append((name, condition))
    print(f"[{status}] {name}" + (f" — {detail}" if detail and not condition else ""), flush=True)


def windows():
    out = subprocess.run(
        ["xdotool", "search", "--name", "AI Monitor"], capture_output=True, text=True
    ).stdout.split()
    return [w for w in out if w.strip()]


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


def layout_labels(proxy):
    rev, layout = proxy.call_sync(
        "GetLayout",
        GLib.Variant("(iias)", (0, -1, [])),
        Gio.DBusCallFlags.NONE,
        5000,
        None,
    ).unpack()
    # layout: (id, properties a{sv}, children av); each child same shape.
    children = layout[2]
    labels = []
    for child in children:
        props = child[1]
        raw = props.get("label")
        label = raw.unpack() if isinstance(raw, GLib.Variant) else (raw or "---")
        labels.append(label or "---")
    return labels, [c[0] for c in children]


def click(proxy, item_id):
    proxy.call_sync(
        "Event",
        GLib.Variant("(isvu)", (item_id, "clicked", GLib.Variant("s", ""), 0)),
        Gio.DBusCallFlags.NONE,
        5000,
        None,
    )


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
    proc = subprocess.Popen(
        [binary], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True
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

        menu_path, menu = None, None
        ok = until(
            lambda: _try_menu(conn, item), 10, "dbusmenu available"
        )
        state = _try_menu(conn, item)
        check("dbusmenu available", state is not None)
        if state:
            menu_path, menu = state
            labels, ids = layout_labels(menu)
            # freshness/activity are dynamic (health/proposal suffixes);
            # assert the static prefix, the info tail and the action block.
            check(
                "dbusmenu labels match macOS menu",
                len(labels) == 14
                and labels[:3] == EXPECTED_HEAD
                and all(labels[i] for i in (3, 4))
                and labels[5:] == EXPECTED_TAIL,
                f"got {labels}",
            )
            by_label = {labels[i]: ids[i] for i in range(len(labels))}
        else:
            by_label = {}

        rss = []
        check("no window at startup", not windows(), f"windows: {windows()}")
        rss.append(("initial", rss_kb(proc.pid)))

        def open_and_hide(label_open, label_hide, cycles=1):
            for _ in range(cycles):
                click(menu, by_label[label_open])
                ok = until(lambda: len(windows()) == 1, 15, f"open via {label_open}")
                check(f"{label_open} opens one window", ok, f"windows: {windows()}")
                rss.append(("open", rss_kb(proc.pid)))
                click(menu, by_label[label_hide])
                ok = until(
                    lambda: not windows() and proc.poll() is None,
                    15,
                    f"hide via {label_hide}",
                )
                check(f"{label_hide} hides, process alive", ok)
                rss.append(("hide", rss_kb(proc.pid)))

        open_and_hide("Abrir painel", "Ocultar painel", cycles=3)
        open_and_hide("Saúde e sessões", "Ocultar painel")

        # Compositor close (title bar / WM_DELETE) → window gone, app stays.
        click(menu, by_label["Abrir painel"])
        check(
            "window open before compositor close",
            until(lambda: len(windows()) == 1, 15, "reopen for close test"),
        )
        close_request()
        check(
            "compositor close destroys window, process alive",
            until(lambda: not windows() and proc.poll() is None, 15, "WM close"),
        )
        rss.append(("wm-close", rss_kb(proc.pid)))
        click(menu, by_label["Abrir painel"])
        check(
            "reopen after compositor close",
            until(lambda: len(windows()) == 1, 15, "reopen"),
        )
        click(menu, by_label["Ocultar painel"])
        until(lambda: not windows(), 15, "hide after reopen")

        # Second instance: forwards Show and exits 0; first reopens.
        second = subprocess.run([binary], capture_output=True, text=True, timeout=15)
        check("second instance exits 0", second.returncode == 0, second.stderr[:300])
        check(
            "second instance reopened the dashboard",
            until(lambda: len(windows()) == 1, 15, "Show forwarded"),
            f"windows: {windows()}",
        )
        rss.append(("reopen", rss_kb(proc.pid)))

        click(menu, by_label["Sair do AI Monitor"])
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
