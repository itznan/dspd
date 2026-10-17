// SPDX-License-Identifier: MIT
// Copyright (c) 2026 itznan

//! Embedded styles and theme definitions for HTML benchmark reports.

/// Complete dark-themed CSS styling matching the CrystalDiskMark visual layout.
pub const REPORT_CSS: &str = r#"
    :root {
      --bg: #0d1117;
      --card-bg: #161b22;
      --border: #30363d;
      --text: #c9d1d9;
      --text-muted: #8b949e;
      --read-color: #58a6ff;
      --write-color: #3fb950;
      --accent: #f0883e;
      --font-mono: 'Consolas', 'Courier New', monospace;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body {
      background-color: var(--bg);
      color: var(--text);
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif;
      padding: 2.5rem;
      line-height: 1.5;
    }
    .container { max-width: 960px; margin: 0 auto; }
    .header {
      border-bottom: 1px solid var(--border);
      padding-bottom: 1.5rem;
      margin-bottom: 2rem;
    }
    h1 { font-size: 2rem; color: #fff; margin-bottom: 0.5rem; }
    .meta-grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
      gap: 1rem;
      margin-top: 1rem;
    }
    .meta-item {
      background: var(--card-bg);
      padding: 0.75rem 1rem;
      border-radius: 6px;
      border: 1px solid var(--border);
    }
    .meta-label { font-size: 0.75rem; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px; }
    .meta-value { font-size: 1rem; font-weight: 600; color: #fff; margin-top: 0.25rem; }
    h2 { font-size: 1.25rem; color: #fff; margin: 2rem 0 1rem; display: flex; align-items: center; gap: 0.5rem; }
    table {
      width: 100%;
      border-collapse: collapse;
      background: var(--card-bg);
      border-radius: 6px;
      overflow: hidden;
      border: 1px solid var(--border);
      margin-bottom: 1.5rem;
    }
    th {
      background: #21262d;
      color: var(--text);
      font-weight: 600;
      text-align: right;
      padding: 0.85rem 1.2rem;
      font-size: 0.85rem;
      border-bottom: 1px solid var(--border);
    }
    th:first-child { text-align: left; }
    td {
      padding: 0.85rem 1.2rem;
      text-align: right;
      font-family: var(--font-mono);
      font-size: 0.95rem;
      border-bottom: 1px solid #21262d;
    }
    td:first-child {
      text-align: left;
      font-weight: 600;
      font-family: inherit;
      color: #fff;
    }
    tr:last-child td { border-bottom: none; }
    tr:hover td { background: rgba(255, 255, 255, 0.02); }
    .read-val { color: var(--read-color); font-weight: 600; }
    .write-val { color: var(--write-color); font-weight: 600; }
    .footer {
      margin-top: 3rem;
      padding-top: 1.5rem;
      border-top: 1px solid var(--border);
      font-size: 0.85rem;
      color: var(--text-muted);
      text-align: center;
    }
    .footer a { color: var(--read-color); text-decoration: none; }
    .footer a:hover { text-decoration: underline; }
"#;
