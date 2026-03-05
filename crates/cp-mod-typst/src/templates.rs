use std::fs;
use std::path::Path;

/// Directory where built-in Typst templates are seeded and stored.
const TEMPLATES_DIR: &str = ".context-pilot/shared/typst-templates";

/// Seed built-in templates to .context-pilot/shared/typst-templates/ if they don't already exist.
pub fn seed() {
    let _r1 = fs::create_dir_all(TEMPLATES_DIR);

    let templates = [("report", REPORT_TEMPLATE), ("invoice", INVOICE_TEMPLATE), ("letter", LETTER_TEMPLATE)];

    for (name, content) in &templates {
        let path = format!("{TEMPLATES_DIR}/{name}.typ");
        if !Path::new(&path).exists() {
            let _r2 = fs::write(&path, content);
        }
    }
}

/// List available template names from the templates directory.
#[must_use]
pub fn list_template_names() -> Vec<String> {
    let templates_dir = Path::new(TEMPLATES_DIR);
    if !templates_dir.exists() {
        return Vec::new();
    }
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(templates_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "typ")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                names.push(stem.to_string());
            }
        }
    }
    names.sort();
    names
}

/// Built-in Typst report template with title page, table of contents, and paginated layout.
const REPORT_TEMPLATE: &str = r##"// Report Template — Context Pilot
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
"##;

/// Built-in Typst invoice template with company/client details and itemized billing table.
const INVOICE_TEMPLATE: &str = r##"// Invoice Template — Context Pilot
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
"##;

/// Built-in Typst letter template with sender/recipient addresses and formal layout.
const LETTER_TEMPLATE: &str = r##"// Letter Template — Context Pilot
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
"##;
