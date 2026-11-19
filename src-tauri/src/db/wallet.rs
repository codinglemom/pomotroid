use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::queries::unix_now;

// Every amount crossing IPC, including lifetime totals, fits a JS integer.
pub const MAX_MINOR: i64 = 9_007_199_254_740_991;
pub const RATE_KEY: &str = "reward_rate_minor_per_hour";
type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Serialize)]
pub struct WalletSummary {
    pub rate_minor_per_hour: Option<i64>,
    pub balance_minor: i64,
    pub income_minor: i64,
    pub today_income_minor: i64,
    pub expense_minor: i64,
}

#[derive(Debug, Serialize)]
pub struct WalletDailyIncome {
    pub date: String,
    pub income_minor: i64,
}

#[derive(Debug, Serialize)]
pub struct WalletIncomeHistory {
    pub as_of_local_date: String,
    pub previous_income_minor: i64,
    pub days: Vec<WalletDailyIncome>,
}

#[derive(Debug, Serialize)]
pub struct LedgerEntry {
    pub id: i64,
    pub source_session_id: Option<i64>,
    pub kind: String,
    pub amount_minor: i64,
    pub created_at: i64,
    pub note: String,
    pub duration_secs: Option<u32>,
    pub rate_minor_per_hour: Option<i64>,
    pub started_at: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct LedgerPage {
    pub entries: Vec<LedgerEntry>,
    pub has_more: bool,
}

fn db_error(error: rusqlite::Error) -> String {
    format!("wallet_database: {error}")
}

fn positive_money(value: i64) -> Result<()> {
    if !(1..=MAX_MINOR).contains(&value) {
        return Err("wallet_invalid_amount".into());
    }
    Ok(())
}

pub fn get_rate(conn: &Connection) -> Result<Option<i64>> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key=?1", [RATE_KEY], |r| {
            r.get(0)
        })
        .optional()
        .map_err(db_error)?;
    raw.map(|v| {
        let rate = v
            .parse::<i64>()
            .map_err(|_| "wallet_invalid_amount".to_string())?;
        positive_money(rate)?;
        Ok(rate)
    })
    .transpose()
}

pub fn set_rate(conn: &Connection, rate: i64) -> Result<()> {
    positive_money(rate)?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![RATE_KEY, rate.to_string()],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn summary(conn: &Connection) -> Result<WalletSummary> {
    let (income, expense, today_income): (i64, i64, i64) = conn
        .query_row(
            "SELECT COALESCE(SUM(CASE WHEN kind='income' THEN amount_minor ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN kind='expense' THEN -amount_minor ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN kind='income'
                    AND date(created_at, 'unixepoch', 'localtime') = date('now', 'localtime')
                    THEN amount_minor ELSE 0 END), 0)
         FROM reward_ledger",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(db_error)?;
    if !(0..=MAX_MINOR).contains(&income) || !(0..=income).contains(&expense) {
        return Err("wallet_amount_limit".into());
    }
    Ok(WalletSummary {
        rate_minor_per_hour: get_rate(conn)?,
        balance_minor: income - expense,
        income_minor: income,
        today_income_minor: today_income,
        expense_minor: expense,
    })
}

pub fn income_history(conn: &Connection, days: u32) -> Result<WalletIncomeHistory> {
    income_history_at(conn, days, unix_now())
}

fn income_history_at(conn: &Connection, days: u32, now: i64) -> Result<WalletIncomeHistory> {
    if !matches!(days, 7 | 30) {
        return Err("wallet_invalid_request".into());
    }
    let as_of_local_date: String = conn
        .query_row("SELECT date(?1, 'unixepoch', 'localtime')", [now], |r| {
            r.get(0)
        })
        .map_err(db_error)?;
    let mut stmt = conn
        .prepare(
            "WITH RECURSIVE dates(day, offset) AS (
                SELECT date(?1, printf('-%d days', ?2)), 0
                UNION ALL
                SELECT date(day, '+1 day'), offset + 1 FROM dates WHERE offset < ?2
             )
             SELECT dates.day, COALESCE(ledger.amount_minor, 0)
             FROM dates LEFT JOIN reward_ledger AS ledger
               ON ledger.kind='income'
               AND date(ledger.created_at, 'unixepoch', 'localtime')=dates.day
             ORDER BY dates.day, ledger.id",
        )
        .map_err(db_error)?;
    let mut rows = stmt
        .query(params![as_of_local_date, days])
        .map_err(db_error)?;
    let mut daily: Vec<WalletDailyIncome> = Vec::with_capacity(days as usize + 1);
    // Add integers in Rust so a SQLite SUM overflow cannot become a rounded amount.
    while let Some(row) = rows.next().map_err(db_error)? {
        let date: String = row.get(0).map_err(db_error)?;
        let amount: i64 = row.get(1).map_err(db_error)?;
        if !(0..=MAX_MINOR).contains(&amount) {
            return Err("wallet_amount_limit".into());
        }
        if let Some(day) = daily.last_mut().filter(|day| day.date == date) {
            day.income_minor = day
                .income_minor
                .checked_add(amount)
                .filter(|total| *total <= MAX_MINOR)
                .ok_or("wallet_amount_limit")?;
        } else {
            daily.push(WalletDailyIncome {
                date,
                income_minor: amount,
            });
        }
    }
    let previous_income_minor = daily.remove(0).income_minor;
    daily.iter().try_fold(0_i64, |total, day| {
        total
            .checked_add(day.income_minor)
            .filter(|total| *total <= MAX_MINOR)
            .ok_or("wallet_amount_limit")
    })?;
    Ok(WalletIncomeHistory {
        as_of_local_date,
        previous_income_minor,
        days: daily,
    })
}

fn reward_minor(duration: u32, rate: i64) -> Result<i64> {
    positive_money(rate)?;
    let amount = i128::from(duration)
        .checked_mul(i128::from(rate))
        .and_then(|v| v.checked_add(1800))
        .map(|v| v / 3600)
        .filter(|v| *v <= i128::from(MAX_MINOR))
        .ok_or_else(|| "wallet_amount_limit".to_string())?;
    Ok(amount as i64)
}

/// Finalize only an open session. Completion and its reward commit together.
/// Returns true only when a new ledger entry was committed.
pub fn complete_session(conn: &Connection, id: i64, completed: bool) -> Result<bool> {
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    let session = tx
        .query_row(
            "SELECT round_type, duration_secs, reward_rate_minor_per_hour, started_at
         FROM sessions WHERE id=?1 AND ended_at IS NULL AND completed=0",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()
        .map_err(db_error)?;
    let Some((round, duration, rate, started_at)) = session else {
        return Ok(false);
    };
    let now = unix_now();
    tx.execute(
        "UPDATE sessions SET ended_at=?1, completed=?2 WHERE id=?3",
        params![now, completed, id],
    )
    .map_err(db_error)?;
    let mut credited = false;
    if let (true, "work", Some(rate)) = (completed, round.as_str(), rate) {
        let amount = reward_minor(duration, rate)?;
        let totals = summary(&tx)?;
        if totals
            .income_minor
            .checked_add(amount)
            .filter(|v| *v <= MAX_MINOR)
            .is_none()
        {
            return Err("wallet_amount_limit".into());
        }
        // UNIQUE(source_session_id) also guards against duplicate settlement.
        tx.execute(
            "INSERT INTO reward_ledger
             (source_session_id, kind, amount_minor, created_at, duration_secs, rate_minor_per_hour, started_at)
             VALUES (?1, 'income', ?2, ?3, ?4, ?5, ?6)",
            params![id, amount, now, duration, rate, started_at],
        ).map_err(db_error)?;
        credited = true;
    }
    tx.commit().map_err(db_error)?;
    Ok(credited)
}

/// request_id allows a retry after a lost IPC response without double spending.
pub fn spend(conn: &Connection, amount: i64, note: &str, request_id: &str) -> Result<()> {
    positive_money(amount)?;
    let note = note.trim();
    if note.is_empty() || note.chars().count() > 500 {
        return Err("wallet_invalid_note".into());
    }
    if request_id.is_empty() || request_id.len() > 100 {
        return Err("wallet_invalid_request".into());
    }
    let tx = conn.unchecked_transaction().map_err(db_error)?;
    let existing: Option<(i64, String)> = tx
        .query_row(
            "SELECT amount_minor, note FROM reward_ledger WHERE request_id=?1",
            [request_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    if let Some((saved, saved_note)) = existing {
        if saved != -amount || saved_note != note {
            return Err("wallet_invalid_request".into());
        }
        tx.commit().map_err(db_error)?;
        return Ok(());
    }
    let totals = summary(&tx)?;
    if amount > totals.balance_minor {
        return Err("wallet_insufficient_balance".into());
    }
    if totals
        .expense_minor
        .checked_add(amount)
        .filter(|v| *v <= MAX_MINOR)
        .is_none()
    {
        return Err("wallet_amount_limit".into());
    }
    tx.execute(
        "INSERT INTO reward_ledger (request_id, kind, amount_minor, created_at, note)
         VALUES (?1, 'expense', ?2, ?3, ?4)",
        params![request_id, -amount, unix_now(), note],
    )
    .map_err(db_error)?;
    tx.commit().map_err(db_error)?;
    Ok(())
}

pub fn ledger(conn: &Connection, before_id: Option<i64>) -> Result<LedgerPage> {
    if before_id.is_some_and(|v| !(1..=MAX_MINOR).contains(&v)) {
        return Err("wallet_invalid_request".into());
    }
    let mut stmt = conn
        .prepare(
            "SELECT id, source_session_id, kind, amount_minor, created_at, note,
                duration_secs, rate_minor_per_hour, started_at FROM reward_ledger
         WHERE (?1 IS NULL OR id < ?1) ORDER BY id DESC LIMIT 51",
        )
        .map_err(db_error)?;
    let mut entries = stmt
        .query_map([before_id], |r| {
            Ok(LedgerEntry {
                id: r.get(0)?,
                source_session_id: r.get(1)?,
                kind: r.get(2)?,
                amount_minor: r.get(3)?,
                created_at: r.get(4)?,
                note: r.get(5)?,
                duration_secs: r.get(6)?,
                rate_minor_per_hour: r.get(7)?,
                started_at: r.get(8)?,
            })
        })
        .map_err(db_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db_error)?;
    let has_more = entries.len() > 50;
    entries.truncate(50);
    Ok(LedgerPage { entries, has_more })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{migrations, queries};

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn session(conn: &Connection, round: &str, seconds: u32, rate: Option<i64>) -> i64 {
        queries::insert_session_with_reward_rate(conn, round, seconds, rate).unwrap()
    }

    fn local_timestamp(conn: &Connection, datetime: &str) -> i64 {
        conn.query_row("SELECT unixepoch(?1, 'utc')", [datetime], |r| r.get(0))
            .unwrap()
    }

    fn calendar_timestamp(conn: &Connection, now: i64, offset: i32) -> i64 {
        conn.query_row(
            "SELECT unixepoch(date(?1, 'unixepoch', 'localtime', printf('%+d days', ?2))
                || ' 12:00:00', 'utc')",
            params![now, offset],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn post_income(conn: &Connection, created_at: i64, amount: i64) {
        conn.execute(
            "INSERT INTO reward_ledger (kind, amount_minor, created_at)
             VALUES ('income', ?1, ?2)",
            params![amount, created_at],
        )
        .unwrap();
    }

    #[test]
    fn income_history_fills_local_calendar_days_in_order_including_today() {
        let conn = setup();
        let now = local_timestamp(&conn, "2024-03-02 12:00:00");
        for (count, first) in [(7, "2024-02-25"), (30, "2024-02-02")] {
            let history = income_history_at(&conn, count, now).unwrap();
            assert_eq!(history.as_of_local_date, "2024-03-02");
            assert_eq!(history.previous_income_minor, 0);
            assert_eq!(history.days.len(), count as usize);
            assert_eq!(history.days[0].date, first);
            assert_eq!(history.days.last().unwrap().date, history.as_of_local_date);
            assert!(history.days.iter().all(|day| day.income_minor == 0));
            assert!(history.days.iter().any(|day| day.date == "2024-02-29"));
            for pair in history.days.windows(2) {
                let next: String = conn
                    .query_row("SELECT date(?1, '+1 day')", [&pair[0].date], |r| r.get(0))
                    .unwrap();
                assert_eq!(pair[1].date, next);
                assert!(pair[0].date < pair[1].date);
            }
        }
    }

    #[test]
    fn income_history_separates_previous_day_and_excludes_earlier_and_future_dates() {
        for count in [7, 30] {
            let conn = setup();
            let now = local_timestamp(&conn, "2024-03-11 12:00:00");
            post_income(
                &conn,
                calendar_timestamp(&conn, now, -(count as i32) - 1),
                900,
            );
            post_income(&conn, calendar_timestamp(&conn, now, -(count as i32)), 200);
            post_income(&conn, calendar_timestamp(&conn, now, 1 - count as i32), 100);
            post_income(&conn, calendar_timestamp(&conn, now, 1), 800);
            let history = income_history_at(&conn, count, now).unwrap();
            assert_eq!(history.previous_income_minor, 200);
            assert_eq!(history.days[0].income_minor, 100);
            assert_eq!(
                history.days.iter().map(|day| day.income_minor).sum::<i64>(),
                100
            );
        }
    }

    #[test]
    fn income_history_uses_local_posting_midnight_and_not_session_start_or_rate() {
        let conn = setup();
        let midnight = local_timestamp(&conn, "2024-03-11 00:00:00");
        post_income(&conn, midnight - 1, 125);
        post_income(&conn, midnight, 300);
        conn.execute(
            "UPDATE reward_ledger SET started_at=?1, duration_secs=3600,
                rate_minor_per_hour=9999 WHERE created_at=?2",
            params![calendar_timestamp(&conn, midnight, -4), midnight],
        )
        .unwrap();
        let before = income_history_at(&conn, 7, midnight - 1).unwrap();
        assert_eq!(before.as_of_local_date, "2024-03-10");
        assert_eq!(before.days.last().unwrap().income_minor, 125);
        let after = income_history_at(&conn, 7, midnight + 1).unwrap();
        assert_eq!(after.as_of_local_date, "2024-03-11");
        assert_eq!(after.days[5].income_minor, 125);
        assert_eq!(after.days[6].income_minor, 300);
        set_rate(&conn, 700).unwrap();
        let changed_rate = income_history_at(&conn, 7, midnight + 1).unwrap();
        assert_eq!(changed_rate.days[6].income_minor, 300);
    }

    #[test]
    fn income_history_accumulates_beyond_ledger_page_and_ignores_expenses() {
        let conn = setup();
        let now = local_timestamp(&conn, "2024-03-11 12:00:00");
        for _ in 0..103 {
            post_income(&conn, now, 3);
        }
        post_income(&conn, now, 0);
        conn.execute(
            "INSERT INTO reward_ledger (kind, amount_minor, created_at)
             VALUES ('expense', -200, ?1)",
            [now],
        )
        .unwrap();
        assert_eq!(ledger(&conn, None).unwrap().entries.len(), 50);
        assert_eq!(
            income_history_at(&conn, 7, now).unwrap().days[6].income_minor,
            309
        );
    }

    #[test]
    fn income_history_rejects_unsupported_ranges() {
        let conn = setup();
        for count in [0, 1, 6, 8, 29, 31, u32::MAX] {
            assert_eq!(
                income_history_at(&conn, count, 0).unwrap_err(),
                "wallet_invalid_request"
            );
        }
    }

    #[test]
    fn income_history_enforces_daily_and_selected_range_safe_integer_limits() {
        let conn = setup();
        let now = local_timestamp(&conn, "2024-03-11 12:00:00");
        let previous = calendar_timestamp(&conn, now, -7);
        let yesterday = calendar_timestamp(&conn, now, -1);
        // The comparison day has its own limit; it is not part of the selected range total.
        post_income(&conn, previous, MAX_MINOR);
        post_income(&conn, now, MAX_MINOR - 1);
        post_income(&conn, yesterday, 1);
        let history = income_history_at(&conn, 7, now).unwrap();
        assert_eq!(history.previous_income_minor, MAX_MINOR);
        assert_eq!(history.days[6].income_minor, MAX_MINOR - 1);
        post_income(&conn, yesterday, 1);
        assert_eq!(
            income_history_at(&conn, 7, now).unwrap_err(),
            "wallet_amount_limit"
        );

        conn.execute("DELETE FROM reward_ledger", []).unwrap();
        post_income(&conn, now, MAX_MINOR);
        post_income(&conn, now, 1);
        assert_eq!(
            income_history_at(&conn, 7, now).unwrap_err(),
            "wallet_amount_limit"
        );

        conn.execute("DELETE FROM reward_ledger", []).unwrap();
        post_income(&conn, previous, MAX_MINOR);
        post_income(&conn, previous, 1);
        assert_eq!(
            income_history_at(&conn, 7, now).unwrap_err(),
            "wallet_amount_limit"
        );

        conn.execute("DELETE FROM reward_ledger", []).unwrap();
        post_income(&conn, now, i64::MAX);
        post_income(&conn, now, i64::MAX);
        assert_eq!(
            income_history_at(&conn, 7, now).unwrap_err(),
            "wallet_amount_limit"
        );
    }

    #[test]
    fn completion_is_idempotent_and_skipped_or_historical_sessions_never_earn() {
        let conn = setup();
        for (round, completed, rate) in [
            ("short-break", true, Some(2400)),
            ("long-break", true, Some(2400)),
            ("work", false, Some(2400)),
            ("work", true, None),
        ] {
            let id = session(&conn, round, 1500, rate);
            assert!(!complete_session(&conn, id, completed).unwrap());
            assert!(!complete_session(&conn, id, true).unwrap());
        }
        let abandoned = session(&conn, "work", 1500, Some(2400));
        assert!(abandoned > 0);
        assert_eq!(summary(&conn).unwrap().income_minor, 0);
        let id = session(&conn, "work", 1500, Some(2400));
        assert!(complete_session(&conn, id, true).unwrap());
        assert!(!complete_session(&conn, id, true).unwrap());
        assert!(!complete_session(&conn, id, false).unwrap());
        assert_eq!(summary(&conn).unwrap().income_minor, 1000);
    }

    #[test]
    fn snapshots_are_immutable_and_round_half_up_to_minor_units() {
        let conn = setup();
        assert_eq!(get_rate(&conn).unwrap(), None);
        let no_rate = session(&conn, "work", 3600, get_rate(&conn).unwrap());
        set_rate(&conn, 1).unwrap();
        let id = session(&conn, "work", 1800, get_rate(&conn).unwrap());
        set_rate(&conn, 2400).unwrap();
        complete_session(&conn, no_rate, true).unwrap();
        complete_session(&conn, id, true).unwrap();
        assert_eq!(summary(&conn).unwrap().income_minor, 1);
        assert_eq!(summary(&conn).unwrap().today_income_minor, 1);
        let entry = &ledger(&conn, None).unwrap().entries[0];
        assert_eq!(entry.rate_minor_per_hour, Some(1));
        assert_eq!(entry.duration_secs, Some(1800));
        assert_eq!(reward_minor(1799, 1).unwrap(), 0);
        assert_eq!(reward_minor(1500, 1000).unwrap(), 417);
        assert!(reward_minor(u32::MAX, MAX_MINOR).is_err());
        assert_eq!(reward_minor(3600, MAX_MINOR).unwrap(), MAX_MINOR);
    }

    #[test]
    fn today_income_uses_local_posting_date_and_ignores_expenses_and_old_sessions() {
        let conn = setup();
        assert_eq!(summary(&conn).unwrap().today_income_minor, 0);
        let historical = session(&conn, "work", 3600, None);
        set_rate(&conn, 100).unwrap();
        assert!(!complete_session(&conn, historical, true).unwrap());
        assert_eq!(summary(&conn).unwrap().today_income_minor, 0);

        // Construct UTC timestamps from local calendar boundaries, including DST.
        let midnight: i64 = conn
            .query_row(
                "SELECT unixepoch(date('now', 'localtime'), 'utc')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let yesterday = session(&conn, "work", 3600, Some(100));
        complete_session(&conn, yesterday, true).unwrap();
        conn.execute(
            "UPDATE reward_ledger SET created_at=?1 WHERE source_session_id=?2",
            params![midnight - 1, yesterday],
        )
        .unwrap();
        assert_eq!(summary(&conn).unwrap().today_income_minor, 0);

        let today = session(&conn, "work", 3600, Some(300));
        complete_session(&conn, today, true).unwrap();
        // Posting time counts even if the work started on the preceding date.
        conn.execute(
            "UPDATE reward_ledger SET created_at=?1, started_at=?2 WHERE source_session_id=?3",
            params![midnight, midnight - 3600, today],
        )
        .unwrap();
        spend(&conn, 50, "Tea", "today").unwrap();
        set_rate(&conn, 9999).unwrap();
        let totals = summary(&conn).unwrap();
        assert_eq!(totals.today_income_minor, 300);
        assert_eq!(
            (
                totals.income_minor,
                totals.expense_minor,
                totals.balance_minor
            ),
            (400, 50, 350)
        );
        assert_eq!(ledger(&conn, None).unwrap().entries.len(), 3);
    }

    #[test]
    fn ledger_failure_rolls_back_completion_and_can_be_retried() {
        let conn = setup();
        let id = session(&conn, "work", 3600, Some(100));
        conn.execute_batch("CREATE TRIGGER fail_reward BEFORE INSERT ON reward_ledger BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert!(complete_session(&conn, id, true).is_err());
        let row: (i64, Option<i64>) = conn
            .query_row(
                "SELECT completed, ended_at FROM sessions WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, (0, None));
        assert_eq!(summary(&conn).unwrap().income_minor, 0);
        conn.execute_batch("DROP TRIGGER fail_reward").unwrap();
        assert!(complete_session(&conn, id, true).unwrap());
    }

    #[test]
    fn expenses_validate_balance_notes_and_idempotency() {
        let conn = setup();
        let id = session(&conn, "work", 3600, Some(1000));
        complete_session(&conn, id, true).unwrap();
        for bad in [0, -1, MAX_MINOR + 1] {
            assert!(spend(&conn, bad, "Tea", "bad").is_err());
            assert!(set_rate(&conn, bad).is_err());
        }
        assert!(spend(&conn, 1001, "Tea", "over").is_err());
        assert!(spend(&conn, 100, "  ", "empty").is_err());
        assert!(spend(&conn, 100, &"a".repeat(501), "long").is_err());
        assert!(spend(&conn, 100, "Tea", "").is_err());
        spend(&conn, 400, " Tea ", "one").unwrap();
        spend(&conn, 400, "Tea", "one").unwrap();
        assert!(spend(&conn, 401, "Tea", "one").is_err());
        assert_eq!(summary(&conn).unwrap().balance_minor, 600);
        spend(&conn, 600, "Book", "two").unwrap();
        assert_eq!(summary(&conn).unwrap().expense_minor, 1000);
        assert!(spend(&conn, 1, "Tea", "three").is_err());
    }

    #[test]
    fn lifetime_total_limit_rejects_income_atomically() {
        let conn = setup();
        let first = session(&conn, "work", 3600, Some(MAX_MINOR));
        complete_session(&conn, first, true).unwrap();
        let second = session(&conn, "work", 3600, Some(1));
        assert!(complete_session(&conn, second, true).is_err());
        assert_eq!(summary(&conn).unwrap().income_minor, MAX_MINOR);
        assert_eq!(
            conn.query_row(
                "SELECT completed FROM sessions WHERE id=?1",
                [second],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn pagination_exposes_every_entry_without_duplicates() {
        let conn = setup();
        for _ in 0..103 {
            let id = session(&conn, "work", 3600, Some(1));
            complete_session(&conn, id, true).unwrap();
        }
        let mut before = None;
        let mut ids = Vec::new();
        loop {
            let page = ledger(&conn, before).unwrap();
            ids.extend(page.entries.iter().map(|e| e.id));
            if !page.has_more {
                break;
            }
            before = page.entries.last().map(|e| e.id);
        }
        assert_eq!(ids, (1..=103).rev().collect::<Vec<_>>());
    }

    #[test]
    fn file_reopen_and_clearing_history_or_settings_preserve_wallet() {
        let path = std::env::temp_dir().join(format!(
            "pomotroid-wallet-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
            migrations::run(&conn).unwrap();
            set_rate(&conn, 2400).unwrap();
            let id = session(&conn, "work", 1500, get_rate(&conn).unwrap());
            complete_session(&conn, id, true).unwrap();
            spend(&conn, 300, "Coffee", "persist").unwrap();
            conn.execute("DELETE FROM sessions", []).unwrap();
            conn.execute("DELETE FROM settings", []).unwrap();
        }
        {
            let conn = Connection::open(&path).unwrap();
            migrations::run(&conn).unwrap();
            let totals = summary(&conn).unwrap();
            assert_eq!(
                (
                    totals.balance_minor,
                    totals.income_minor,
                    totals.expense_minor
                ),
                (700, 1000, 300)
            );
            assert_eq!(totals.rate_minor_per_hour, None);
            let page = ledger(&conn, None).unwrap();
            assert_eq!(page.entries.len(), 2);
            assert_eq!(page.entries[1].duration_secs, Some(1500));
            assert_eq!(page.entries[1].rate_minor_per_hour, Some(2400));
            assert!(page.entries[1].started_at.is_some());
            // AUTOINCREMENT prevents cleared session IDs from colliding.
            let new_id = session(&conn, "work", 3600, Some(100));
            assert!(complete_session(&conn, new_id, true).unwrap());
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn clearing_history_preserves_active_round_and_its_reward() {
        let conn = setup();
        let old = session(&conn, "work", 60, Some(600));
        complete_session(&conn, old, true).unwrap();
        let abandoned = session(&conn, "work", 60, Some(600));
        let active = session(&conn, "work", 60, Some(600));
        assert_eq!(
            queries::clear_session_history(&conn, Some(active)).unwrap(),
            2
        );
        let remaining: Vec<i64> = conn
            .prepare("SELECT id FROM sessions")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(remaining, vec![active]);
        assert!(!complete_session(&conn, abandoned, true).unwrap());
        assert!(complete_session(&conn, active, true).unwrap());
        assert_eq!(summary(&conn).unwrap().income_minor, 20);
        assert!(!complete_session(&conn, active, true).unwrap());
        // A stale active ID must not preserve an already completed session.
        assert_eq!(
            queries::clear_session_history(&conn, Some(active)).unwrap(),
            1
        );
        assert_eq!(summary(&conn).unwrap().income_minor, 20);
        let next = session(&conn, "work", 60, Some(600));
        assert!(next > active);
        assert_eq!(queries::clear_session_history(&conn, None).unwrap(), 1);
    }
}
