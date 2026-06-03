// Invoice Template — Context Pilot
// Usage: #import "../templates/invoice.typ": *

#let invoice(
  company: "Your Company",
  company_address: "123 Main St, City, Country",
  client: "Client Name",
  client_address: "456 Other St, City, Country",
  invoice_number: "INV-001",
  date: datetime.today().display(),
  due_date: "30 days",
  items: (),
  body,
) = {
  set page(paper: "a4", margin: 2.5cm)
  set text(font: "Liberation Serif", size: 10pt)

  // Header
  grid(
    columns: (1fr, 1fr),
    align(left)[
      #text(size: 18pt, weight: "bold")[#company]
      #v(0.3em)
      #text(size: 9pt, fill: rgb("#666666"))[#company_address]
    ],
    align(right)[
      #text(size: 24pt, weight: "bold", fill: rgb("#2563eb"))[INVOICE]
      #v(0.3em)
      #text(size: 9pt)[
        *Invoice:* #invoice_number \
        *Date:* #date \
        *Due:* #due_date
      ]
    ],
  )

  line(length: 100%, stroke: 0.5pt + rgb("#dddddd"))
  v(1em)

  // Client info
  text(size: 9pt, fill: rgb("#666666"))[*Bill To:*]
  v(0.3em)
  text(weight: "bold")[#client]
  linebreak()
  text(size: 9pt, fill: rgb("#666666"))[#client_address]

  v(1.5em)

  // Items table
  if items.len() > 0 {
    let total = items.map(item => item.at(2)).sum()
    table(
      columns: (1fr, auto, auto, auto),
      inset: 8pt,
      stroke: 0.5pt + rgb("#dddddd"),
      table.header(
        [*Description*], [*Qty*], [*Unit Price*], [*Amount*],
      ),
      ..items.map(item => (
        item.at(0),
        align(center)[#item.at(1)],
        align(right)[#item.at(2)],
        align(right)[#calc.round(item.at(1) * item.at(2), digits: 2)],
      )).flatten(),
      table.footer(
        [], [], [*Total:*], align(right)[*#calc.round(total, digits: 2)*],
      ),
    )
  }

  v(1em)
  body
}
