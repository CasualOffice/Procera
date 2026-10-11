//! XLSX for grid exports (FR-V07, A-35): one sheet with exactly the header and cells the grid shows,
//! in its order. Text is always written as a string cell, never as a formula; text that a spreadsheet
//! would read as a formula when retyped (`= + - @`, tab, CR) also carries Excel's quote prefix, the
//! XLSX twin of the CSV apostrophe (docs/06 §5).

use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use rust_xlsxwriter::{Format, Workbook, Worksheet, XlsxError};
use serde_json::Value;

/// The request body limit for `POST /exports` (the XLSX cells travel in it).
pub const BODY_MAX: usize = 8 * 1024 * 1024;
/// Rows per synchronous export; larger sets wait for async exports (docs/05 §4).
pub const MAX_ROWS: usize = 10_000;
/// Excel's limit for one cell.
const MAX_CELL_CHARS: usize = 32_767;

pub const CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

fn formula_like(s: &str) -> bool {
    s.starts_with(['=', '+', '-', '@', '\t', '\r'])
}

fn text(
    ws: &mut Worksheet,
    row: u32,
    col: u16,
    s: &str,
    plain: &Format,
    quoted: &Format,
) -> Result<(), XlsxError> {
    ws.write_string_with_format(row, col, s, if formula_like(s) { quoted } else { plain })?;
    Ok(())
}

/// Builds the workbook, or says why the request is not a well-formed grid (shown as a 422 detail).
pub fn workbook(
    header: &[String],
    cells: &[Vec<Value>],
    columns: usize,
    rows: u32,
) -> Result<Vec<u8>, String> {
    if header.is_empty() || header.len() != columns {
        return Err("header must name each exported column".into());
    }
    if cells.len() > MAX_ROWS {
        return Err(format!("at most {MAX_ROWS} rows per export"));
    }
    if cells.len() != rows as usize {
        return Err("rows must equal the number of exported rows".into());
    }
    if cells.iter().any(|r| r.len() != columns) {
        return Err("each row needs one value per exported column".into());
    }
    build(header, cells).map_err(|e| match e {
        Bad::Cell(m) => m,
        Bad::Xlsx(e) => {
            tracing::error!(error = %e, "xlsx export failed");
            "the workbook could not be built".into()
        }
    })
}

enum Bad {
    Cell(String),
    Xlsx(XlsxError),
}

impl From<XlsxError> for Bad {
    fn from(e: XlsxError) -> Self {
        Bad::Xlsx(e)
    }
}

fn build(header: &[String], cells: &[Vec<Value>]) -> Result<Vec<u8>, Bad> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Export")?;
    let plain = Format::new();
    let quoted = Format::new().set_quote_prefix();
    let bold = Format::new().set_bold();
    let bold_quoted = Format::new().set_bold().set_quote_prefix();
    for (c, h) in header.iter().enumerate() {
        if h.chars().count() > MAX_CELL_CHARS {
            return Err(Bad::Cell("a header is longer than a spreadsheet cell allows".into()));
        }
        text(ws, 0, col(c), h, &bold, &bold_quoted)?;
    }
    for (r, row) in cells.iter().enumerate() {
        let r = u32::try_from(r + 1).map_err(|_| Bad::Cell("too many rows".into()))?;
        for (c, v) in row.iter().enumerate() {
            match v {
                Value::Null => {}
                Value::Bool(b) => {
                    ws.write_boolean(r, col(c), *b)?;
                }
                Value::Number(n) => {
                    ws.write_number(r, col(c), n.as_f64().unwrap_or_default())?;
                }
                Value::String(s) if s.chars().count() <= MAX_CELL_CHARS => {
                    text(ws, r, col(c), s, &plain, &quoted)?
                }
                Value::String(_) => {
                    return Err(Bad::Cell("a value is longer than a spreadsheet cell allows".into()));
                }
                _ => return Err(Bad::Cell("cells are text, numbers, booleans or null".into())),
            }
        }
    }
    ws.set_freeze_panes(1, 0)?;
    ws.autofit();
    Ok(wb.save_to_buffer()?)
}

/// Column index (the caller caps columns at 100).
fn col(c: usize) -> u16 {
    u16::try_from(c).unwrap_or(u16::MAX)
}

pub fn response(list: &str, bytes: Vec<u8>) -> Response {
    let mut res = bytes.into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(CONTENT_TYPE));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    // `list` is already checked to be `[a-z0-9_.-]`.
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{}.xlsx\"", list.replace('.', "-")))
    {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    res
}
