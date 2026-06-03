// Report Template — Context Pilot
// Usage: #import "../templates/report.typ": *

#let report(
  title: "Report Title",
  author: "Author Name",
  date: datetime.today().display(),
  body,
) = {
  // Page setup
  set page(
    paper: "a4",
    margin: (top: 3cm, bottom: 2.5cm, left: 2.5cm, right: 2.5cm),
    header: context {
      if counter(page).get().first() > 1 {
        align(right, text(size: 9pt, fill: rgb("#888888"))[#title])
      }
    },
    footer: align(center, text(size: 9pt, fill: rgb("#888888"))[
      Page #context counter(page).display() of #context counter(page).final().first()
    ]),
  )

  // Typography
  set text(font: "Liberation Serif", size: 11pt)
  set par(justify: true, leading: 0.65em)
  set heading(numbering: "1.1")

  // Title page
  align(center + horizon)[
    #text(size: 28pt, weight: "bold")[#title]
    #v(1em)
    #text(size: 14pt, fill: rgb("#555555"))[#author]
    #v(0.5em)
    #text(size: 12pt, fill: rgb("#888888"))[#date]
  ]

  pagebreak()

  // Table of contents
  outline(indent: auto)
  pagebreak()

  // Body
  body
}

// Export: wrap your document in report(title: "...", author: "...")[...]
