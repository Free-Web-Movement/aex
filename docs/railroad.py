#!/usr/bin/env python3
"""Railroad-diagram renderer (bottlecaps.de/rr/ui style).

Produces:
  docs/Web_Straight_Line.png    -- wrapped (multi-row) HTTP request line
  docs/aex_architecture.png     -- Aex server state machine (one rule per row)

Node kinds: Term / NonTerm / Seq / Choice / Opt / Rep.
Each element has a horizontal "main line"; `measure` -> (w, h, baseline).
Connector rails use rounded right-angle elbows.
"""
import math
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch, Rectangle
from matplotlib.lines import Line2D

matplotlib.rcParams["font.family"] = "DejaVu Sans"

FS = 10.5
BOX_H = 24
PAD_X = 10
HGAP = 26
VGAP = 12
VROW = 32
BYPASS = 15
ALT_PAD = 16
CURVE_R = 9

LINE = "#3a3a3a"
DASH = dict(linestyle=(0, (2, 3)), linewidth=1.2, color=LINE)

# theme (swapped per diagram)
TERM_FILL = "#eaf2ff"
TERM_EDGE = "#4a6fa5"
NON_FILL = "#f3f3f3"
NON_EDGE = "#8c8c8c"


def set_theme(term_fill, term_edge, non_fill="#f3f3f3", non_edge="#8c8c8c"):
    global TERM_FILL, TERM_EDGE, NON_FILL, NON_EDGE
    TERM_FILL, TERM_EDGE, NON_FILL, NON_EDGE = term_fill, term_edge, non_fill, non_edge


def text_w(s):
    from matplotlib.textpath import TextPath
    from matplotlib.font_manager import FontProperties
    fp = FontProperties(family="DejaVu Sans", weight="bold")
    tp = TextPath((0, 0), s, size=FS, prop=fp)
    return tp.get_extents().width * (100.0 / 72.0)


class Node: pass
class Term(Node):
    def __init__(self, s): self.s = s
class NonTerm(Node):
    def __init__(self, s): self.s = s
class Seq(Node):
    def __init__(self, *items): self.items = [i for i in items if i is not None]
class Choice(Node):
    def __init__(self, *alts): self.alts = [a for a in alts if a is not None]
class Opt(Node):
    def __init__(self, item): self.item = item
class Rep(Node):
    def __init__(self, item): self.item = item


def measure(n):
    if isinstance(n, (Term, NonTerm)):
        return (text_w(n.s) + 2 * PAD_X, BOX_H, BOX_H / 2.0)
    if isinstance(n, Seq):
        ms = [measure(i) for i in n.items]
        w = sum(m[0] for m in ms) + HGAP * (len(ms) - 1)
        U = max(m[2] for m in ms)
        D = max(m[1] - m[2] for m in ms)
        return (w, U + D, U)
    if isinstance(n, Choice):
        ms = [measure(a) for a in n.alts]
        w = max(m[0] for m in ms) + 2 * ALT_PAD
        h = sum(m[1] for m in ms) + VGAP * (len(ms) - 1)
        return (w, h, ms[0][2])
    if isinstance(n, Opt):
        w, h, b = measure(n.item)
        return (w + 2 * ALT_PAD, h + BYPASS, b + BYPASS)
    if isinstance(n, Rep):
        w, h, b = measure(n.item)
        return (w + 2 * ALT_PAD, h + BYPASS, b)
    raise TypeError(n)


def poly(ax, pts, **kw):
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    ax.add_line(Line2D(xs, ys, color=kw.get("color", LINE),
                       lw=kw.get("lw", 1.4), solid_capstyle="round",
                       solid_joinstyle="round", zorder=2,
                       linestyle=kw.get("linestyle", "-")))


def rpoly(ax, pts, r=CURVE_R, **kw):
    """Like `poly`, but replace each 90° corner with a quarter-circle arc."""
    if len(pts) < 3:
        poly(ax, pts, **kw)
        return
    out = [pts[0]]
    for i in range(1, len(pts) - 1):
        ax_, ay = pts[i - 1]
        vx, vy = pts[i]
        bx, by = pts[i + 1]
        inx, iny = vx - ax_, vy - ay
        oux, ouy = bx - vx, by - vy
        lin = math.hypot(inx, iny)
        lout = math.hypot(oux, ouy)
        if lin == 0 or lout == 0:
            out.append((vx, vy))
            continue
        u1 = (-inx / lin, -iny / lin)   # V -> A
        u2 = (oux / lout, ouy / lout)   # V -> B
        if abs(u1[0] * u2[1] - u1[1] * u2[0]) < 1e-6:
            out.append((vx, vy))        # collinear, no corner
            continue
        rr = min(r, lin / 2, lout / 2)
        p1 = (vx + u1[0] * rr, vy + u1[1] * rr)
        p2 = (vx + u2[0] * rr, vy + u2[1] * rr)
        cx = p1[0] + u2[0] * rr
        cy = p1[1] + u2[1] * rr
        a1 = math.atan2(p1[1] - cy, p1[0] - cx)
        a2 = math.atan2(p2[1] - cy, p2[0] - cx)
        da = a2 - a1
        while da > math.pi:
            da -= 2 * math.pi
        while da < -math.pi:
            da += 2 * math.pi
        for s in range(1, 10):
            a = a1 + da * s / 9.0
            out.append((cx + rr * math.cos(a), cy + rr * math.sin(a)))
        out.append(p2)
    out.append(pts[-1])
    poly(ax, out, **kw)


def hline(ax, x1, x2, y):
    poly(ax, [(x1, y), (x2, y)])


def draw(n, ax, x, y):
    if isinstance(n, Term):
        w = text_w(n.s) + 2 * PAD_X
        ax.add_patch(FancyBboxPatch((x, y - BOX_H / 2), w, BOX_H,
                     boxstyle="round,pad=0,rounding_size=8", zorder=3,
                     linewidth=1.2, edgecolor=TERM_EDGE, facecolor=TERM_FILL))
        ax.text(x + w / 2, y, n.s, ha="center", va="center", fontsize=FS,
                color="#1a1a1a", weight="bold", zorder=4)
        return w
    if isinstance(n, NonTerm):
        w = text_w(n.s) + 2 * PAD_X
        ax.add_patch(Rectangle((x, y - BOX_H / 2), w, BOX_H, linewidth=1.0,
                     edgecolor=NON_EDGE, facecolor=NON_FILL, zorder=3))
        ax.text(x + w / 2, y, n.s, ha="center", va="center", fontsize=FS,
                color="#222222", zorder=4)
        return w
    if isinstance(n, Seq):
        cx = x
        prev_right = None
        for item in n.items:
            wi = draw(item, ax, cx, y)
            if prev_right is not None:
                hline(ax, prev_right, cx, y)
            prev_right = cx + wi
            cx += wi + HGAP
        return cx - HGAP - x
    if isinstance(n, Choice):
        ms = [measure(a) for a in n.alts]
        inner = max(m[0] for m in ms)
        w = inner + 2 * ALT_PAD
        ys = []
        cur = y - ms[0][2]
        for (aw, ah, ab) in ms:
            ys.append(cur + ab)
            cur += ah + VGAP
        for (aw, ah, ab), yl, alt in zip(ms, ys, n.alts):
            draw(alt, ax, x + ALT_PAD, yl)
            rpoly(ax, [(x, y), (x, yl), (x + ALT_PAD, yl)])            # left elbow
            rpoly(ax, [(x + ALT_PAD + aw, yl), (x + w, yl), (x + w, y)])  # right elbow
        return w
    if isinstance(n, Opt):
        iw, ih, ib = measure(n.item)
        w = iw + 2 * ALT_PAD
        draw(n.item, ax, x + ALT_PAD, y)
        hline(ax, x, x + ALT_PAD, y)
        hline(ax, x + ALT_PAD + iw, x + w, y)
        top = y - ib - BYPASS
        rpoly(ax, [(x, y), (x, top), (x + w, top), (x + w, y)])
        return w
    if isinstance(n, Rep):
        iw, ih, ib = measure(n.item)
        w = iw + 2 * ALT_PAD
        draw(n.item, ax, x + ALT_PAD, y)
        hline(ax, x, x + ALT_PAD, y)
        hline(ax, x + ALT_PAD + iw, x + w, y)
        bot = y + (ih - ib) + BYPASS
        rpoly(ax, [(x, y), (x, bot), (x + w, bot), (x + w, y)])
        return w
    raise TypeError(n)


# ---------------------------------------------------------------- row renderer
def render_rows(rows, path, title=None, mark="::="):
    lbl_w = max([text_w(name) for name, _ in rows] + [text_w(mark)]) + 84
    rm = [(name, tree, measure(tree)) for name, tree in rows]
    total_w = lbl_w + max(m[0] for _, _, m in rm)
    total_h = sum(m[1] for _, _, m in rm) + VROW * (len(rows) - 1)

    fig, ax = plt.subplots(figsize=((total_w + 90) / 100.0,
                                    (total_h + 100) / 100.0), dpi=150)
    fig.subplots_adjust(left=0, right=1, top=1, bottom=0)
    y_top = 34
    for name, tree, (w, h, b) in rm:
        baseline = y_top + b
        if name:
            ax.text(8, baseline, name, ha="left", va="center", fontsize=FS + 0.5,
                    color="#123")
        if mark:
            ax.text(lbl_w - 34, baseline, mark, ha="right", va="center",
                    fontsize=FS, color="#555")
        draw(tree, ax, lbl_w, baseline)
        y_top += h + VROW
    if title:
        ax.set_title(title, fontsize=13, loc="center", pad=16)
    ax.set_xlim(0, total_w + 90)
    ax.set_ylim(y_top + 20, 0)
    ax.axis("off")
    fig.savefig(path, bbox_inches="tight", facecolor="white")
    plt.close(fig)
    print("wrote", path)


# ---------------------------------------------------------------- wrapped line
def render_wrapped(items, path, title=None, nrows=3):
    per = math.ceil(len(items) / nrows)
    rows = [items[i:i + per] for i in range(0, len(items), per)]

    layouts = []
    for row in rows:
        ms = [measure(i) for i in row]
        w = sum(m[0] for m in ms) + HGAP * (len(ms) - 1)
        U = max(m[2] for m in ms)
        D = max(m[1] - m[2] for m in ms)
        layouts.append((row, ms, w, U, D))
    max_w = max(l[2] for l in layouts)

    margin_l = 60
    margin_r = 60
    x0 = margin_l
    right_x = x0 + max_w + 24
    total_w = right_x + margin_r
    total_h = sum(l[3] + l[4] for l in layouts) + VROW * (len(layouts) - 1)

    fig, ax = plt.subplots(figsize=((total_w) / 100.0, (total_h + 110) / 100.0),
                           dpi=150)
    fig.subplots_adjust(left=0, right=1, top=1, bottom=0)
    y_top = 30
    for idx, (row, ms, rw, U, D) in enumerate(layouts):
        yl = y_top + U
        # main line across the row
        hline(ax, x0, right_x, yl)
        # items
        cx = x0
        prev = None
        for it, m in zip(row, ms):
            draw(it, ax, cx, yl)
            prev = cx + m[0]
            cx += m[0] + HGAP
        # start / end / wrap markers
        if idx == 0:
            ax.plot([x0 - 14], [yl], marker=">", ms=11, color=LINE)
        else:
            ax.add_line(Line2D([x0 - 34, x0 - 6], [yl, yl], **DASH))
        if idx == len(layouts) - 1:
            ax.plot([right_x + 8], [yl], marker="<", ms=11, color=LINE)
        else:
            ax.add_line(Line2D([right_x + 6, right_x + 34], [yl, yl], **DASH))
        y_top += U + D + VROW

    if title:
        ax.set_title(title, fontsize=13, loc="center", pad=18)
    ax.set_xlim(0, total_w)
    ax.set_ylim(y_top + 10, 0)
    ax.axis("off")
    fig.savefig(path, bbox_inches="tight", facecolor="white")
    plt.close(fig)
    print("wrote", path)


# ---------------------------------------------------------------- grammars
def web_straight_line():
    return [
        Choice(Term("Request"), Term("http"), Term("websocket")),
        Choice(Term("routing"), Seq(Term("method"), Term("url"))),
        Choice(Term("authorization"), Term("cookie"), Term("session"),
               Term("http-auth")),
        Choice(Term("data parsing"), Term("query"), Term("body"),
               Term("formdata"), Term("files")),
        Choice(Term("deeper access control"),
               Seq(Choice(Term("policy"), Term("isLogined"), Term("isOwner"),
                          Term("isAdmin"), Term("...")), Term("fallbacks"))),
        Choice(Term("data control"), Term("pagination"), Term("filter"),
               Term("cache"), Term("i18n")),
        Choice(Term("error processing"), Term("http"), Term("application"),
               Term("database"), Term("business")),
        Choice(Term("business logic"),
               Seq(Term("command"), Term("process")),
               Seq(Term("modelling"), Term("database"))),
        Choice(Term("output"),
               Seq(Term("displayable"), Choice(Term("html"), Term("xml"))),
               Seq(Term("api"), Choice(Term("json"), Term("rest"),
                                       Term("graphql"))),
               Term("streaming"), Term("...")),
        Choice(Term("middleware"), Term("linear executor"),
               Term("false => short-circuit")),
        Term("Response"),
    ]


def aex_server_state_machine():
    machine = Seq(
        NonTerm("Receive"), NonTerm("Parse"), NonTerm("Route"),
        NonTerm("Middleware"), NonTerm("Command"), NonTerm("WriteBack"))
    return [
        ("Aex_Server_State_Machine", Seq(
            Term("Server"), NonTerm("DetectorRegistry"),
            Rep(NonTerm("Machine")), Term("start"), NonTerm("address"))),
        ("Machine", Rep(machine)),
        ("Receive", Choice(Term("detect"), Term("command"))),
        ("Parse", Term("decode")),
        ("Route", Choice(Term("method + url"), Term("command + message"))),
        ("Middleware", Term("Executor chain (false => short-circuit)")),
        ("Command", Term("command process")),
        ("WriteBack", Choice(Term("response"), Term("reply"))),
        ("Protocol", Choice(NonTerm("Tcp"), NonTerm("Udp"))),
        ("Tcp", Seq(Term("Tcp"), Opt(NonTerm("RawHandler")),
                    Rep(NonTerm("SubProtocol")), Rep(NonTerm("Middleware")),
                    Rep(NonTerm("Command")))),
        ("Udp", Seq(Term("Udp"), Opt(NonTerm("DatagramHandler")),
                    Rep(NonTerm("Middleware")), Rep(NonTerm("Command")))),
        ("SubProtocol", Choice(NonTerm("Http1"), NonTerm("Http2"),
                               NonTerm("Websocket"), NonTerm("Socks"))),
        ("Http1", Seq(Term("Http1"), Choice(NonTerm("Router"), NonTerm("Handler")),
                      Opt(NonTerm("Websocket")), Rep(NonTerm("Middleware")),
                      Rep(NonTerm("Command")))),
    ]


if __name__ == "__main__":
    import sys
    out = sys.argv[1] if len(sys.argv) > 1 else "/tmp/opencode"
    set_theme("#ffec9e", "#b8860b")  # yellow, like the upstream diagram
    render_wrapped(web_straight_line(), f"{out}/Web_Straight_Line.png",
                   "Web_Straight_Line  —  HTTP request phases (wrapped)", nrows=3)
    set_theme("#eaf2ff", "#4a6fa5")  # blue for the architecture
    render_rows(aex_server_state_machine(), f"{out}/aex_architecture.png",
                "Aex_Server_State_Machine  —  general server state machine")
