//! The probe's checks (Task 2.1 of the Chat state split), one step per frame: change something in
//! the list, notify, wait for the child view to paint, read what the rows recorded. Prints
//! `list-probe: {"g1": …, …}` and exits 0 (a check that can't draw exits 1).
//!
//! G1 follow, G2 anchor, G3 cost, G4 resize, G5 at the bottom, G6 spinners, G7 the kit's bar
//! (Task 6.2); each is an object with
//! `pass` and the numbers behind it.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui::{point, px, AsyncApp, Bounds, Context, Entity, ListOffset, Pixels};
use notchling_kit::ui::ThumbAt;
use serde_json::{json, Value};

use super::list_probe::{ListProbe, ProbeList, LAST, NARROW, ROWS, WORDS_PER_LINE};

/// How far a row may move, or the last row sit from the viewport's bottom, and still count as
/// where it was (points).
const TOLERANCE: f32 = 1.0;
/// The most a frame may wait for the child to paint before the probe gives up.
const FRAME_WAIT: Duration = Duration::from_secs(3);

/// Starts the checks once the window has drawn a few frames.
pub fn start(list: Entity<ProbeList>, cx: &mut Context<ListProbe>) {
    cx.spawn(async move |this, cx| {
        cx.background_executor()
            .timer(Duration::from_millis(1500))
            .await;
        let mut script = Script {
            list,
            outer: this,
            cx: cx.clone(),
        };
        match script.run().await {
            Ok(result) => {
                println!("list-probe: {result}");
                std::process::exit(0);
            }
            Err(why) => {
                eprintln!("list-probe failed: {why}");
                std::process::exit(1);
            }
        }
    })
    .detach();
}

/// What the latest frame painted.
struct Snap {
    viewport: Bounds<Pixels>,
    rows: HashMap<usize, Bounds<Pixels>>,
}

impl Snap {
    /// How far row `ix`'s top is below the viewport's top; `None` if it wasn't painted.
    fn top_of(&self, ix: usize) -> Option<f32> {
        let b = self.rows.get(&ix)?;
        Some(f32::from(b.top() - self.viewport.top()))
    }

    /// The first row with any of it in the viewport.
    fn first_visible(&self) -> Option<usize> {
        let edge = self.viewport.top() + px(0.5);
        self.rows
            .iter()
            .filter(|(_, b)| b.bottom() > edge)
            .map(|(ix, _)| *ix)
            .min()
    }

    /// How far the last row's painted bottom is from the viewport's bottom.
    fn last_gap(&self) -> f32 {
        self.rows.get(&LAST).map_or(f32::INFINITY, |b| {
            f32::from(b.bottom() - self.viewport.bottom()).abs()
        })
    }
}

struct Script {
    list: Entity<ProbeList>,
    outer: gpui::WeakEntity<ListProbe>,
    cx: AsyncApp,
}

type Step<T = ()> = Result<T, String>;

impl Script {
    fn with<R>(&self, f: impl FnOnce(&mut ProbeList, &mut Context<ProbeList>) -> R) -> Step<R> {
        self.cx
            .update(|cx| self.list.update(cx, f))
            .map_err(|e| e.to_string())
    }

    /// Notifies the list's view and waits until it has painted, `n` times.
    async fn tick(&self, n: usize) -> Step {
        for _ in 0..n {
            let before = self.with(|p, cx| {
                cx.notify();
                p.frame.paints.get()
            })?;
            let started = Instant::now();
            while self.with(|p, _| p.frame.paints.get())? <= before {
                if started.elapsed() > FRAME_WAIT {
                    return Err("no frame was drawn (is the display asleep?)".into());
                }
                self.cx
                    .background_executor()
                    .timer(Duration::from_millis(3))
                    .await;
            }
        }
        Ok(())
    }

    fn snap(&self) -> Step<Snap> {
        self.with(|p, _| Snap {
            viewport: p.frame.viewport.get(),
            rows: p.frame.rows.borrow().clone(),
        })
    }

    /// Replaces row `ix` by a row `lines` lines taller, as a change in a message would.
    fn grow(&self, ix: usize, lines: usize) -> Step {
        self.with(|p, _| {
            p.words[ix] += lines * WORDS_PER_LINE;
            p.state.splice(ix..ix + 1, 1);
        })
    }

    /// Back to the end, following.
    async fn to_end(&self) -> Step {
        self.with(|p, _| {
            p.state.scroll_to(ListOffset {
                item_ix: ROWS,
                offset_in_item: px(0.0),
            })
        })?;
        self.tick(3).await
    }

    async fn run(&mut self) -> Step<Value> {
        self.tick(5).await?;
        let first = self.snap()?;
        let height_before = average_height(&first);

        let g1 = self.g1().await?;
        let g2 = self.g2().await?;
        let g3 = self.g3().await?;
        let g5 = self.g5().await?;
        let g4 = self.g4(height_before).await?;
        let g6 = self.g6().await?;
        let g7 = self.g7().await?;
        let frames = self.with(|p, _| p.frame.paints.get())?;
        Ok(
            json!({"g1": g1, "g2": g2, "g3": g3, "g4": g4, "g5": g5, "g6": g6, "g7": g7, "frames": frames}),
        )
    }

    /// G1: while following, the last row grows ten times; its bottom stays on the viewport's.
    async fn g1(&self) -> Step<Value> {
        let (mut painted, mut api, mut api_missing) = (0f32, 0f32, 0);
        let mut last_seen = json!(null);
        for _ in 0..10 {
            self.grow(LAST, 1)?;
            self.tick(1).await?;
            let s = self.snap()?;
            painted = painted.max(s.last_gap());
            last_seen = json!({
                "viewport": [f32::from(s.viewport.top()), f32::from(s.viewport.bottom())],
                "last_row": s.rows.get(&LAST).map(|b| [f32::from(b.top()), f32::from(b.bottom())]),
                "rows_painted": s.rows.len(),
            });
            match self.with(|p, _| p.state.bounds_for_item(LAST))? {
                Some(b) => api = api.max(f32::from(b.bottom() - s.viewport.bottom()).abs()),
                None => api_missing += 1,
            }
        }
        Ok(json!({
            "pass": painted <= TOLERANCE,
            "max_gap_px": painted,
            "bounds_for_item_max_gap_px": api,
            "bounds_for_item_none": api_missing,
            "grows": 10,
            "last_frame": last_seen,
        }))
    }

    /// G2: scrolled up, a splice above the first visible row and a remeasure of that row itself
    /// keep its top where it was; a bare splice of it does not.
    async fn g2(&self) -> Step<Value> {
        self.with(|p, _| p.state.scroll_by(px(-3000.0)))?;
        self.tick(3).await?;
        // Rows never drawn have no height, so a jump from the end can only land inside the rows
        // measured so far: note where, then walk up in wheel-sized steps to rows nobody has seen.
        let landed = self.snap()?.first_visible();
        for _ in 0..60 {
            if self.snap()?.first_visible().is_some_and(|ix| ix <= 240) {
                break;
            }
            self.with(|p, _| p.state.scroll_by(px(-400.0)))?;
            self.tick(1).await?;
        }
        // Part way into a row, so that a splice that resets the offset shows.
        self.with(|p, _| p.state.scroll_by(px(37.0)))?;
        self.tick(3).await?;
        let s = self.snap()?;
        let first = s
            .first_visible()
            .ok_or("no row on screen after scrolling up")?;
        let top = s.top_of(first).unwrap_or(f32::NAN);
        let above = first.saturating_sub(3);

        self.grow(above, 3)?;
        self.tick(2).await?;
        let above_top = self.snap()?.top_of(first);

        self.with(|p, _| {
            let at = p.state.logical_scroll_top();
            p.words[first] += 2 * WORDS_PER_LINE;
            p.state.splice(first..first + 1, 1);
            p.state.scroll_to(at);
        })?;
        self.tick(2).await?;
        let remeasured_top = self.snap()?.top_of(first);

        self.grow(first, 1)?;
        self.tick(2).await?;
        let naive_top = self.snap()?.top_of(first);

        let moved = |t: Option<f32>| t.map_or(f32::INFINITY, |t| (t - top).abs());
        let (a, r) = (moved(above_top), moved(remeasured_top));
        Ok(json!({
            "pass": a <= TOLERANCE && r <= TOLERANCE,
            "landed_row_after_a_3000_pt_jump": landed,
            "anchor_row": first,
            "top_px": top,
            "splice_above_shift_px": a,
            "remeasure_shift_px": r,
            "bare_splice_shift_px": moved(naive_top),
        }))
    }

    /// G3: sixty frames of ±40 pt; the rows the list asks for per frame against the rows on
    /// screen, with the rows laid out only in prepaint (the probe's default) and again in
    /// `request_layout` (the list's default sizing).
    async fn g3(&self) -> Step<Value> {
        let auto = self.scrub().await?;
        self.with(|p, _| p.infer = true)?;
        self.tick(3).await?;
        let infer = self.scrub().await?;
        self.with(|p, _| p.infer = false)?;
        self.tick(3).await?;
        Ok(json!({
            "pass": auto.0 <= auto.1 + 2.0,
            "render_item_per_frame": auto.0,
            "visible_rows_per_frame": auto.1,
            "infer_render_item_per_frame": infer.0,
            "infer_visible_rows_per_frame": infer.1,
            "frames": 60,
        }))
    }

    /// Sixty frames scrolling 40 pt up and down: (rows asked for, rows on screen) per frame.
    async fn scrub(&self) -> Step<(f32, f32)> {
        let calls = self.with(|p, _| p.frame.calls.get())?;
        let mut visible = 0usize;
        for i in 0..60 {
            let by = if i % 2 == 0 { 40.0 } else { -40.0 };
            self.with(|p, _| p.state.scroll_by(px(by)))?;
            self.tick(1).await?;
            visible += self.snap()?.rows.len();
        }
        let calls = self.with(|p, _| p.frame.calls.get())? - calls;
        Ok((calls as f32 / 60.0, visible as f32 / 60.0))
    }

    /// G5: the scroll bar's offset to the middle and then to the end lands at the end.
    async fn g5(&self) -> Step<Value> {
        let max = self.with(|p, _| f32::from(p.state.max_offset_for_scrollbar().height))?;
        self.with(|p, _| {
            p.state
                .set_offset_from_scrollbar(point(px(0.0), px(-(max / 2.0))))
        })?;
        self.tick(3).await?;
        let middle = self.with(|p, _| p.state.logical_scroll_top().item_ix)?;
        let max = self.with(|p, _| f32::from(p.state.max_offset_for_scrollbar().height))?;
        self.with(|p, _| p.state.set_offset_from_scrollbar(point(px(0.0), px(-max))))?;
        self.tick(3).await?;
        let (end, count) =
            self.with(|p, _| (p.state.logical_scroll_top().item_ix, p.state.item_count()))?;
        let gap = self.snap()?.last_gap();
        Ok(json!({
            "pass": end == count || gap <= TOLERANCE,
            "middle_item_ix": middle,
            "end_item_ix": end,
            "item_count": count,
            "last_row_gap_px": gap,
        }))
    }

    /// G4: the column 600 → 480 pt wide; G1 and G2 hold again and every visible row has a height.
    async fn g4(&self, height_before: f32) -> Step<Value> {
        self.cx
            .update(|cx| {
                self.outer.update(cx, |o, cx| {
                    o.width = NARROW;
                    cx.notify();
                })
            })
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        self.to_end().await?;
        let s = self.snap()?;
        let height_after = average_height(&s);
        let smallest = s
            .rows
            .values()
            .map(|b| f32::from(b.size.height))
            .fold(f32::INFINITY, f32::min);
        let g1 = self.g1().await?;
        let g2 = self.g2().await?;
        let ok = |v: &Value| v["pass"] == Value::Bool(true);
        Ok(json!({
            "pass": ok(&g1) && ok(&g2) && smallest > 0.0 && !s.rows.is_empty(),
            "visible_rows": s.rows.len(),
            "smallest_row_height_px": smallest,
            "average_row_height_before_px": height_before,
            "average_row_height_after_px": height_after,
            "g1_again": g1,
            "g2_again": g2,
        }))
    }

    /// G6: after a splice that makes the list lay its rows out twice, the spinner slots recorded
    /// for the frame are the spinners on screen (one).
    async fn g6(&self) -> Step<Value> {
        self.to_end().await?;
        self.tick(2).await?;
        let control = self.with(|p, _| p.spins.len())?;
        self.with(|p, _| {
            p.autoscroll.set(true);
            p.state.splice(LAST..LAST + 1, 1);
        })?;
        self.tick(1).await?;
        let retried = self.with(|p, _| p.spins.len())?;
        self.with(|p, _| p.autoscroll.set(false))?;
        self.to_end().await?;
        let after = self.with(|p, _| p.spins.len())?;
        Ok(json!({
            "pass": retried == 1,
            "slots_in_a_plain_frame": control,
            "slots_after_an_autoscroll_retry": retried,
            "slots_a_plain_frame_later": after,
        }))
    }
}

impl Script {
    /// G7: the kit's bar over the list: while the list follows its end, the thumb's bottom is at
    /// the track's bottom; scrolled up it is not, and it is back at the bottom after the way down.
    async fn g7(&self) -> Step<Value> {
        self.to_end().await?;
        self.tick(2).await?;
        let following = self.with(|p, _| p.thumb.get())?;
        let gap =
            |t: Option<ThumbAt>| t.map_or(f32::INFINITY, |t| (t.track_bottom - t.bottom).abs());
        let end_gap = gap(following);
        // Grow the last row while following: the thumb stays at the end.
        self.grow(LAST, 1)?;
        self.tick(2).await?;
        let grown_gap = gap(self.with(|p, _| p.thumb.get())?);
        // Up from the end. A list at its end counts as scrolled to the end of its content, a
        // viewport past the last scroll position, so the first step has to be longer than that.
        self.with(|p, _| p.state.scroll_by(px(-1500.0)))?;
        self.tick(3).await?;
        let up = self.with(|p, _| p.thumb.get())?;
        let up_gap = gap(up);
        self.to_end().await?;
        self.tick(2).await?;
        let back_gap = gap(self.with(|p, _| p.thumb.get())?);
        Ok(json!({
            "pass": end_gap <= TOLERANCE && grown_gap <= TOLERANCE && back_gap <= TOLERANCE
                && up_gap > TOLERANCE,
            "following_gap_px": end_gap,
            "following_while_growing_gap_px": grown_gap,
            "scrolled_up_gap_px": up_gap,
            "back_at_the_end_gap_px": back_gap,
            "thumb_scrolled_up": up.map(|t| json!([t.top, t.bottom, t.track_bottom])),
            "thumb_when_following": following.map(|t| json!([t.top, t.bottom, t.track_bottom])),
        }))
    }
}

fn average_height(s: &Snap) -> f32 {
    if s.rows.is_empty() {
        return 0.0;
    }
    s.rows
        .values()
        .map(|b| f32::from(b.size.height))
        .sum::<f32>()
        / s.rows.len() as f32
}
