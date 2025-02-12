use std::collections::VecDeque;

use crossterm::event::{Event, KeyCode};
use ratatui::{widgets::{Axis, GraphType}, style::Style, text::Span};

use crate::app::update_value_i;

use super::{DisplayMode, GraphConfig, DataSet, Dimension};

use rustfft::{FftPlanner, num_complex::Complex};

#[derive(Default)]
pub struct Spectroscope {
	pub sampling_rate: u32,
	pub buffer_size: u32,
	pub average: u32,
	pub buf: Vec<VecDeque<Vec<f64>>>,
	pub window: bool,
	pub log_y: bool,
}

fn magnitude(c: Complex<f64>) -> f64 {
	let squared = (c.re * c.re) + (c.im * c.im);
	squared.sqrt()
}

// got this from https://github.com/phip1611/spectrum-analyzer/blob/3c079ec2785b031d304bb381ff5f5fe04e6bcf71/src/windows.rs#L40
pub fn hann_window(samples: &[f64]) -> Vec<f64> {
	let mut windowed_samples = Vec::with_capacity(samples.len());
	let samples_len = samples.len() as f64;
	for (i, sample) in samples.iter().enumerate() {
		let two_pi_i = 2.0 * std::f64::consts::PI * i as f64;
		let idontknowthename = (two_pi_i / samples_len).cos();
		let multiplier = 0.5 * (1.0 - idontknowthename);
		windowed_samples.push(sample * multiplier)
	}
	windowed_samples
}

impl DisplayMode for Spectroscope {
	fn from_args(args: &crate::ScopeArgs) -> Self {
		Spectroscope {
			sampling_rate: args.sample_rate,
			buffer_size: args.buffer / (2 * args.channels as u32),
			average: 1, buf: Vec::new(),
			window: false,
			log_y: true,
		}
	}

	fn mode_str(&self) -> &'static str {
		"spectro"
	}

	fn channel_name(&self, index: usize) -> String {
		match index {
			0 => "L".into(),
			1 => "R".into(),
			_ => format!("{}", index),
		}
	}

	fn header(&self, _: &GraphConfig) -> String {
		let window_marker = if self.window { "-|-" } else { "---" };
		if self.average <= 1 {
			format!("live  {}  {:.3}Hz bins", window_marker, self.sampling_rate as f64 / self.buffer_size as f64)
		} else {
			format!(
				"{}x avg ({:.1}s)  {}  {:.3}Hz bins",
				self.average,
				(self.average * self.buffer_size) as f64 / self.sampling_rate as f64,
				window_marker,
				self.sampling_rate as f64 / (self.buffer_size * self.average) as f64,
			)
		}
	}

	fn axis(&self, cfg: &GraphConfig, dimension: Dimension) -> Axis {
		let (name, bounds) = match dimension {
			Dimension::X => ("frequency -", [20.0f64.ln(), ((cfg.samples as f64 / cfg.width as f64) * 20000.0).ln()]),
			Dimension::Y => (
				if self.log_y { "| level" } else { "| amplitude" },
				[0.0, if self.log_y { (cfg.scale as f64 / 10.0).ln() } else { cfg.scale as f64 / 10.0 }]),
				// TODO super arbitraty! wtf! also ugly inline ifs, get this thing together!
		};
		let mut a = Axis::default();
		if cfg.show_ui { // TODO don't make it necessary to check show_ui inside here
			a = a.title(Span::styled(name, Style::default().fg(cfg.labels_color)));
		}
		a.style(Style::default().fg(cfg.axis_color)).bounds(bounds)
	}

	fn process(&mut self, cfg: &GraphConfig, data: &Vec<Vec<f64>>) -> Vec<DataSet> {
		if self.average == 0 { self.average = 1 } // otherwise fft breaks
		if !cfg.pause {
			for (i, chan) in data.iter().enumerate() {
				if self.buf.len() <= i {
					self.buf.push(VecDeque::new());
				}
				self.buf[i].push_back(chan.clone());
				while self.buf[i].len() > self.average as usize {
					self.buf[i].pop_front();
				}
			}
		}

		let mut out = Vec::new();
		let mut planner: FftPlanner<f64> = FftPlanner::new();
		let sample_len = self.buffer_size * self.average;
		let resolution = self.sampling_rate as f64 / sample_len as f64;
		let fft = planner.plan_fft_forward(sample_len as usize);

		for (n, chan_queue) in self.buf.iter().enumerate().rev() {
			let mut chunk = chan_queue.iter().flatten().copied().collect::<Vec<f64>>();
			if self.window {
				chunk = hann_window(chunk.as_slice());
			}
			let mut max_val = *chunk.iter().max_by(|a, b| a.total_cmp(b)).expect("empty dataset?");
			if max_val < 1. { max_val = 1.; }
			let mut tmp : Vec<Complex<f64>> = chunk.iter().map(|x| Complex { re: *x / max_val, im: 0.0 }).collect();
			fft.process(tmp.as_mut_slice());
			out.push(DataSet::new(
				Some(self.channel_name(n)),
				tmp[..=tmp.len() / 2]
					.iter()
					.enumerate()
					.map(|(i,x)| ((i as f64 * resolution).ln(), if self.log_y { magnitude(*x).ln() } else { magnitude(*x) }))
					.collect(),
				cfg.marker_type,
				if cfg.scatter { GraphType::Scatter } else { GraphType::Line },
				cfg.palette(n),
			));
		}

		out
	}

	fn handle(&mut self, event: Event) {
		if let Event::Key(key) = event {
			match key.code {
				KeyCode::PageUp   => update_value_i(&mut self.average, true, 1, 1., 1..65535),
				KeyCode::PageDown => update_value_i(&mut self.average, false, 1, 1., 1..65535),
				KeyCode::Char('w') => self.window = !self.window,
				KeyCode::Char('l') => self.log_y = !self.log_y,
				_ => {}
			}
		}
	}

	fn references(&self, cfg: &GraphConfig) -> Vec<DataSet> {
		let s = if self.log_y { (cfg.scale as f64 / 10.0).ln() } else { cfg.scale as f64 / 10.0 };
		vec![
			DataSet::new(None, vec![(0.0, 0.0), ((cfg.samples as f64).ln(), 0.0)], cfg.marker_type, GraphType::Line, cfg.axis_color), 

			// TODO can we auto generate these? lol...
			DataSet::new(None, vec![(20.0f64.ln(), 0.0), (20.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(30.0f64.ln(), 0.0), (30.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(40.0f64.ln(), 0.0), (40.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(50.0f64.ln(), 0.0), (50.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(60.0f64.ln(), 0.0), (60.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(70.0f64.ln(), 0.0), (70.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(80.0f64.ln(), 0.0), (80.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(90.0f64.ln(), 0.0), (90.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(100.0f64.ln(), 0.0), (100.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(200.0f64.ln(), 0.0), (200.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(300.0f64.ln(), 0.0), (300.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(400.0f64.ln(), 0.0), (400.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(500.0f64.ln(), 0.0), (500.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(600.0f64.ln(), 0.0), (600.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(700.0f64.ln(), 0.0), (700.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(800.0f64.ln(), 0.0), (800.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(900.0f64.ln(), 0.0), (900.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(1000.0f64.ln(), 0.0), (1000.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(2000.0f64.ln(), 0.0), (2000.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(3000.0f64.ln(), 0.0), (3000.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(4000.0f64.ln(), 0.0), (4000.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(5000.0f64.ln(), 0.0), (5000.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(6000.0f64.ln(), 0.0), (6000.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(7000.0f64.ln(), 0.0), (7000.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(8000.0f64.ln(), 0.0), (8000.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(9000.0f64.ln(), 0.0), (9000.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(10000.0f64.ln(), 0.0), (10000.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
			DataSet::new(None, vec![(20000.0f64.ln(), 0.0), (20000.0f64.ln(), s)], cfg.marker_type, GraphType::Line, cfg.axis_color),
		]
	}
}
