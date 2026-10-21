// Unit tests for the OpenUI Lang scanner (./openuiParse). The parser is total:
// it either returns a resolved node table with a `root`, or an error string —
// never throws — because it runs on streamed, possibly half-written model
// output. These cases pin the grammar the ```openui``` block advertises in the
// Send tool description.

import { describe, expect, it } from "vitest"

import { parseOpenUi } from "./openuiParse"

describe("parseOpenUi", () => {
  it("parses a nested tree with refs, props and lists", () => {
    const program = parseOpenUi(
      [
        "# deploy panel",
        'title = Heading("Deploy status", level: 2)',
        'ok    = Badge("3 healthy", tone: ok)',
        "row   = Row([ok], gap: 8)",
        'cpu   = Metric(label: "CPU", value: "64%", delta: -4)',
        'root  = Card([title, row, cpu], title: "prod-eu")',
      ].join("\n"),
    )

    expect(program.ok).toBe(true)
    if (!program.ok) return
    expect(program.root).toBe("root")
    expect(program.nodes.size).toBe(5)
    expect(program.nodes.get("title")?.props).toEqual({ level: 2 })
    expect(program.nodes.get("cpu")?.props).toEqual({ label: "CPU", value: "64%", delta: -4 })
    expect(program.nodes.get("row")?.args[0]).toEqual({ kind: "refs", ids: ["ok"] })
  })

  it("keeps commas and brackets inside quoted strings and lists split correctly", () => {
    const program = parseOpenUi(
      ['a = Text("one, two [three]")', "b = BarChart([1, 4, 2])", "root = Stack([a, b])"].join(
        "\n",
      ),
    )

    expect(program.ok).toBe(true)
    if (!program.ok) return
    expect(program.nodes.get("a")?.args[0]).toEqual({ kind: "value", value: "one, two [three]" })
    expect(program.nodes.get("b")?.args[0]).toEqual({ kind: "numbers", values: [1, 4, 2] })
  })

  it("ignores blank lines and comments", () => {
    const program = parseOpenUi("\n# a comment\n\nroot = Divider()\n")
    expect(program.ok).toBe(true)
  })

  it("parses a bare word as a string property value (tone: ok)", () => {
    const program = parseOpenUi(
      ['b = Badge("3 healthy", tone: ok)', "root = Stack([b])"].join("\n"),
    )
    expect(program.ok).toBe(true)
    if (!program.ok) return
    expect(program.nodes.get("b")?.props).toEqual({ tone: "ok" })
    expect(program.nodes.get("root")?.args[0]).toEqual({ kind: "refs", ids: ["b"] })
  })

  it("rejects a missing root", () => {
    const program = parseOpenUi('a = Text("x")')
    expect(program).toEqual({ ok: false, error: "missing 'root = …' entry" })
  })

  it("rejects an unresolved child reference", () => {
    const program = parseOpenUi("root = Stack([missing])")
    expect(program.ok).toBe(false)
    if (program.ok) return
    expect(program.error).toContain("references unknown 'missing'")
  })

  it("rejects a line with no binding", () => {
    const program = parseOpenUi("just some text")
    expect(program.ok).toBe(false)
    if (program.ok) return
    expect(program.error).toContain("expected 'id = Component(…)'")
  })

  it("rejects a malformed component call", () => {
    const program = parseOpenUi("root = NotACall")
    expect(program.ok).toBe(false)
    if (program.ok) return
    expect(program.error).toContain("invalid component call")
  })
})
