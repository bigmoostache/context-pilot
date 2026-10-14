//! Excel conditional formats → Univer `SHEET_CONDITIONAL_FORMATTING_PLUGIN`
//! rules (T851 phase 3).
//!
//! Univer evaluates the rules itself, so only the definitions are shipped:
//! `{ cfId, ranges, stopIfTrue, rule }` per Excel rule. Covered: cell value
//! comparisons, formulas, text tests, blanks/errors, duplicates/uniques,
//! top/bottom N, above/below average, colour scales and data bars.
//! Not covered: `between`/`notBetween` (umya keeps a single formula per rule),
//! icon sets and time periods — those rules are dropped.

use serde_json::{Map, Value, json};
use umya_spreadsheet::drawing::Theme;
use umya_spreadsheet::{
    ConditionalFormatValueObject, ConditionalFormatValueObjectValues, ConditionalFormatValues,
    ConditionalFormattingOperatorValues, ConditionalFormattingRule, Range, Worksheet,
};

use super::grid::{MAX_COLS, MAX_ROWS};
use super::style::{color_hex, style_data};

/// All convertible rules of one sheet, in Excel priority order.
pub(super) fn sheet_rules(ws: &Worksheet, theme: &Theme) -> Vec<Value> {
    let mut out = Vec::new();
    for block in ws.conditional_formatting_collection() {
        let refs = block.sequence_of_references().range_collection();
        let ranges: Vec<Value> = refs.iter().filter_map(range_json).collect();
        let Some(anchor) = refs.first().and_then(anchor_a1) else {
            continue;
        };
        let mut rules: Vec<&ConditionalFormattingRule> = block.conditional_collection().iter().collect();
        rules.sort_by_key(|r| r.priority());
        for rule in rules {
            if let Some(config) = rule_config(rule, theme, &anchor) {
                out.push(json!({
                    "cfId": format!("cf-{}", out.len()),
                    "ranges": ranges,
                    "stopIfTrue": rule.stop_if_true(),
                    "rule": config,
                }));
            }
        }
    }
    out
}

/// A1-style range → Univer `IRange` (0-based, inclusive); single cells allowed.
fn range_json(range: &Range) -> Option<Value> {
    let r0 = range.coordinate_start_row()?.num();
    let c0 = range.coordinate_start_col()?.num();
    let r1 = range.coordinate_end_row().map_or(r0, umya_spreadsheet::RowReference::num);
    let c1 = range.coordinate_end_col().map_or(c0, umya_spreadsheet::ColumnReference::num);
    if r0 == 0 || c0 == 0 || r0 > MAX_ROWS || c0 > MAX_COLS {
        return None;
    }
    Some(json!({
        "startRow": r0.saturating_sub(1),
        "startColumn": c0.saturating_sub(1),
        "endRow": r1.min(MAX_ROWS).saturating_sub(1),
        "endColumn": c1.min(MAX_COLS).saturating_sub(1),
    }))
}

/// Top-left cell of a range in A1 notation — the cell relative formulas refer to.
fn anchor_a1(range: &Range) -> Option<String> {
    let text = range.range().replace('$', "");
    let first = text.split(':').next().filter(|a| !a.is_empty())?;
    Some(first.to_owned())
}

/// One rule's Univer config; `None` for unsupported rule kinds.
fn rule_config(rule: &ConditionalFormattingRule, theme: &Theme, anchor: &str) -> Option<Value> {
    match *rule.get_type() {
        ConditionalFormatValues::ColorScale => color_scale(rule, theme),
        ConditionalFormatValues::DataBar => data_bar(rule, theme),
        ConditionalFormatValues::IconSet | ConditionalFormatValues::TimePeriod => None,
        ConditionalFormatValues::AboveAverage
        | ConditionalFormatValues::BeginsWith
        | ConditionalFormatValues::CellIs
        | ConditionalFormatValues::ContainsBlanks
        | ConditionalFormatValues::ContainsErrors
        | ConditionalFormatValues::ContainsText
        | ConditionalFormatValues::DuplicateValues
        | ConditionalFormatValues::EndsWith
        | ConditionalFormatValues::Expression
        | ConditionalFormatValues::NotContainsBlanks
        | ConditionalFormatValues::NotContainsErrors
        | ConditionalFormatValues::NotContainsText
        | ConditionalFormatValues::Top10
        | ConditionalFormatValues::UniqueValues => highlight(rule, theme, anchor),
    }
}

/// Highlight-cell rule: subtype-specific fields plus the dxf `style`.
fn highlight(rule: &ConditionalFormattingRule, theme: &Theme, anchor: &str) -> Option<Value> {
    let mut config = highlight_fields(rule, anchor)?;
    let style = rule.style().and_then(|s| style_data(s, theme)).unwrap_or_default();
    let _type = config.insert("type".to_owned(), json!("highlightCell"));
    let _style = config.insert("style".to_owned(), Value::Object(style));
    Some(Value::Object(config))
}

/// `subType` + operator/value for each highlight kind.
fn highlight_fields(rule: &ConditionalFormattingRule, anchor: &str) -> Option<Map<String, Value>> {
    let formula = rule.formula().map(umya_spreadsheet::Formula::address_str).unwrap_or_default();
    let value = match *rule.get_type() {
        ConditionalFormatValues::CellIs => cell_is(rule.operator(), &formula, anchor)?,
        ConditionalFormatValues::Expression => json!({ "subType": "formula", "value": format!("={formula}") }),
        ConditionalFormatValues::DuplicateValues => json!({ "subType": "duplicateValues" }),
        ConditionalFormatValues::UniqueValues => json!({ "subType": "uniqueValues" }),
        ConditionalFormatValues::Top10 => json!({
            "subType": "rank", "isBottom": rule.bottom(), "isPercent": rule.percent(), "value": rule.rank(),
        }),
        ConditionalFormatValues::AboveAverage => json!({ "subType": "average", "operator": average_op(rule) }),
        ConditionalFormatValues::BeginsWith
        | ConditionalFormatValues::ContainsBlanks
        | ConditionalFormatValues::ContainsErrors
        | ConditionalFormatValues::ContainsText
        | ConditionalFormatValues::EndsWith
        | ConditionalFormatValues::NotContainsBlanks
        | ConditionalFormatValues::NotContainsErrors
        | ConditionalFormatValues::NotContainsText => text_rule(rule)?,
        ConditionalFormatValues::ColorScale
        | ConditionalFormatValues::DataBar
        | ConditionalFormatValues::IconSet
        | ConditionalFormatValues::TimePeriod => return None,
    };
    value.as_object().cloned()
}

/// `cellIs` comparison with one operand; `between` kinds are unsupported.
/// A numeric operand maps to Univer's number subtype; any other operand
/// (cell reference, formula, text) becomes an equivalent formula rule.
fn cell_is(op: &ConditionalFormattingOperatorValues, formula: &str, anchor: &str) -> Option<Value> {
    let (operator, symbol) = match *op {
        ConditionalFormattingOperatorValues::Equal => ("equal", "="),
        ConditionalFormattingOperatorValues::NotEqual => ("notEqual", "<>"),
        ConditionalFormattingOperatorValues::GreaterThan => ("greaterThan", ">"),
        ConditionalFormattingOperatorValues::GreaterThanOrEqual => ("greaterThanOrEqual", ">="),
        ConditionalFormattingOperatorValues::LessThan => ("lessThan", "<"),
        ConditionalFormattingOperatorValues::LessThanOrEqual => ("lessThanOrEqual", "<="),
        ConditionalFormattingOperatorValues::Between
        | ConditionalFormattingOperatorValues::NotBetween
        | ConditionalFormattingOperatorValues::BeginsWith
        | ConditionalFormattingOperatorValues::ContainsText
        | ConditionalFormattingOperatorValues::EndsWith
        | ConditionalFormattingOperatorValues::NotContains => return None,
    };
    Some(formula.trim().parse::<f64>().map_or_else(
        |_| json!({ "subType": "formula", "value": format!("={anchor}{symbol}{formula}") }),
        |n| json!({ "subType": "number", "operator": operator, "value": n }),
    ))
}

/// Text / blank / error tests (`CFTextOperator` names match Excel's types).
fn text_rule(rule: &ConditionalFormattingRule) -> Option<Value> {
    let operator = match *rule.get_type() {
        ConditionalFormatValues::BeginsWith => "beginsWith",
        ConditionalFormatValues::EndsWith => "endsWith",
        ConditionalFormatValues::ContainsText => "containsText",
        ConditionalFormatValues::NotContainsText => "notContainsText",
        ConditionalFormatValues::ContainsBlanks => "containsBlanks",
        ConditionalFormatValues::NotContainsBlanks => "notContainsBlanks",
        ConditionalFormatValues::ContainsErrors => "containsErrors",
        ConditionalFormatValues::NotContainsErrors => "notContainsErrors",
        ConditionalFormatValues::AboveAverage
        | ConditionalFormatValues::CellIs
        | ConditionalFormatValues::ColorScale
        | ConditionalFormatValues::DataBar
        | ConditionalFormatValues::DuplicateValues
        | ConditionalFormatValues::Expression
        | ConditionalFormatValues::IconSet
        | ConditionalFormatValues::TimePeriod
        | ConditionalFormatValues::Top10
        | ConditionalFormatValues::UniqueValues => return None,
    };
    Some(json!({ "subType": "text", "operator": operator, "value": rule.text() }))
}

/// Above/below (or equal to) the average.
fn average_op(rule: &ConditionalFormattingRule) -> &'static str {
    match (rule.above_average(), rule.equal_average()) {
        (true, false) => "greaterThan",
        (true, true) => "greaterThanOrEqual",
        (false, false) => "lessThan",
        (false, true) => "lessThanOrEqual",
    }
}

/// Colour scale: one stop per value object, paired with its colour.
fn color_scale(rule: &ConditionalFormattingRule, theme: &Theme) -> Option<Value> {
    let scale = rule.color_scale()?;
    let stops: Vec<Value> = scale
        .cfvo_collection()
        .iter()
        .zip(scale.color_collection())
        .enumerate()
        .map(|(index, (cfvo, color))| {
            json!({
                "index": index,
                "color": color_hex(color, theme).unwrap_or_else(|| "#FFFFFF".to_owned()),
                "value": value_config(cfvo),
            })
        })
        .collect();
    (stops.len() >= 2).then(|| json!({ "type": "colorScale", "config": stops }))
}

/// Data bar: min/max value objects and the bar colour.
fn data_bar(rule: &ConditionalFormattingRule, theme: &Theme) -> Option<Value> {
    let data_bar = rule.data_bar()?;
    let mut cfvo = data_bar.cfvo_collection().iter();
    let (min, max) = (cfvo.next()?, cfvo.next()?);
    let color =
        data_bar.color_collection().first().and_then(|c| color_hex(c, theme)).unwrap_or_else(|| "#638EC6".to_owned());
    Some(json!({
        "type": "dataBar",
        "isShowValue": true,
        "config": {
            "min": value_config(min),
            "max": value_config(max),
            "isGradient": true,
            "positiveColor": color,
            "nativeColor": "#FF0000",
        },
    }))
}

/// Excel `cfvo` → Univer `IValueConfig`.
fn value_config(cfvo: &ConditionalFormatValueObject) -> Value {
    let raw = cfvo.val();
    let number = || raw.parse::<f64>().map_or(Value::Null, |n| json!(n));
    let (kind, value) = match *cfvo.get_type() {
        ConditionalFormatValueObjectValues::Min => ("min", Value::Null),
        ConditionalFormatValueObjectValues::Max => ("max", Value::Null),
        ConditionalFormatValueObjectValues::Percent => ("percent", number()),
        ConditionalFormatValueObjectValues::Percentile => ("percentile", number()),
        ConditionalFormatValueObjectValues::Number => ("num", number()),
        ConditionalFormatValueObjectValues::Formula => ("formula", json!(format!("={raw}"))),
    };
    json!({ "type": kind, "value": value })
}
