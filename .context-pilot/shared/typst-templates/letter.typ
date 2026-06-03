// Letter Template — Context Pilot
// Usage: #import "../templates/letter.typ": *

#let letter(
  sender: "Your Name",
  sender_address: "123 Main St\nCity, Country",
  recipient: "Recipient Name",
  recipient_address: "456 Other St\nCity, Country",
  date: datetime.today().display(),
  subject: none,
  body,
) = {
  set page(paper: "a4", margin: (top: 3cm, bottom: 2.5cm, left: 2.5cm, right: 2.5cm))
  set text(font: "Liberation Serif", size: 11pt)
  set par(justify: true, leading: 0.65em)

  // Sender
  align(right)[
    #text(weight: "bold")[#sender]
    #linebreak()
    #text(size: 9pt, fill: rgb("#666666"))[#sender_address]
  ]

  v(2em)

  // Date
  align(right)[#date]

  v(1em)

  // Recipient
  text(weight: "bold")[#recipient]
  linebreak()
  text(size: 9pt, fill: rgb("#666666"))[#recipient_address]

  v(2em)

  // Subject
  if subject != none {
    text(weight: "bold")[Re: #subject]
    v(1em)
  }

  // Salutation
  [Dear #recipient,]
  v(0.5em)

  // Body
  body

  v(2em)

  // Signature
  [Sincerely,]
  v(2em)
  text(weight: "bold")[#sender]
}
