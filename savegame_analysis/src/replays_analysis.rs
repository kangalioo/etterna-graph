use std::path::PathBuf;
use itertools::{izip, Itertools};
use rayon::prelude::*;
use pyo3::prelude::*;
use crate::ok_or_continue;
use crate::util::split_newlines;


static OFFSET_BUCKET_RANGE: u64 = 180;
static NUM_OFFSET_BUCKETS: u64 = 2 * OFFSET_BUCKET_RANGE + 1;

#[pyclass]
#[derive(Default)]
pub struct ReplaysAnalysis {
	#[pyo3(get)]
	pub score_indices: Vec<u64>,
	#[pyo3(get)]
	pub manipulations: Vec<f64>,
	#[pyo3(get)]
	pub deviation_mean: f64, // mean of all non-cb offsets
	#[pyo3(get)]
	pub notes_per_column: [u64; 4],
	#[pyo3(get)]
	pub cbs_per_column: [u64; 4],
	#[pyo3(get)]
	pub longest_mcombo: (u64, String), // contains longest combo and the associated scorekey
	#[pyo3(get)]
	pub offset_buckets: Vec<u64>,
	#[pyo3(get)]
	pub sub_93_offset_buckets: Vec<u64>,
	#[pyo3(get)]
	pub standard_deviation: f64,
	#[pyo3(get)]
	pub fastest_combo: FastestComboInfo,
}

#[pyclass]
#[derive(Default, Clone)]
pub struct FastestComboInfo {
	#[pyo3(get)]
	pub length: u64,
	#[pyo3(get)]
	pub nps: f64,
	#[pyo3(get)]
	pub scorekey: String,
}

#[derive(Default)]
struct ScoreAnalysis {
	// a percentage (from 0.0 to 1.0) that says how many notes were hit out of order
	manipulation: f64,
	// the thing that's called "mean" in Etterna eval screen, except that it only counts non-CBs
	deviation_mean: f64,
	// the number of notes counted for the deviation_mean
	num_deviation_notes: u64,
	// number of total notes for each column
	notes_per_column: [u64; 4],
	// number of combo-breakers for each column
	cbs_per_column: [u64; 4],
	// the length of the longest combo of marvelous-only hits
	longest_mcombo: u64,
	// a vector of size NUM_OFFSET_BUCKETS. Each number corresponds to a certain timing window,
	// for example the middle entry is for (-0.5ms - 0.5ms). each number stands for the number of
	// hits in the respective timing window
	offset_buckets: Vec<u64>,
	// like offset_buckets, but for, well, sub 93% scores only
	sub_93_offset_buckets: Vec<u64>,
	
	fastest_combo: FastestComboInScoreInfo,
}

#[derive(Default)]
struct FastestComboInScoreInfo {
	length: u64,
	nps: f64,
}

// Analyze a single score's replay
fn analyze(path: &str, wifescore: f64, timing_info: &crate::TimingInfo, rate: f64) -> Option<ScoreAnalysis> {
	let bytes = std::fs::read(path).ok()?;
	let approx_max_num_lines = bytes.len() / 18; // 18 is a pretty good value for this
	
	let mut score = ScoreAnalysis::default();
	
	let mut prev_tick: u64 = 0;
	let mut mcombo: u64 = 0;
	let mut num_notes: u64 = 0; // we can't derive this from notes_per_column cuz those exclude 5k+
	let mut num_deviation_notes: u64 = 0; // number of notes used in deviation calculation
	let mut num_manipped_notes: u64 = 0;
	let mut deviation_sum: f64 = 0.0;
	let mut offset_buckets = vec![0u64; NUM_OFFSET_BUCKETS as usize];
	let mut sub_93_offset_buckets = vec![0u64; NUM_OFFSET_BUCKETS as usize];
	let mut ticks = Vec::with_capacity(approx_max_num_lines);
	let mut are_cbs = Vec::with_capacity(approx_max_num_lines);
	for line in split_newlines(&bytes, 5) {
		if line.len() == 0 || line[0usize] == b'H' { continue }
		
		let mut token_iter = line.splitn(3, |&c| c == b' ');
		
		let tick = token_iter.next().expect("Missing tick token");
		let tick: u64 = ok_or_continue!(btoi::btou(tick));
		let deviation = token_iter.next().expect("Missing tick token");
		let deviation: f64 = ok_or_continue!(lexical::parse_lossy(&deviation));
		// "column" has the remainer of the string, luckily we just care about the first column
		let column = token_iter.next().expect("Missing tick token");
		let column: u64 = (column[0] - b'0') as u64;
		
		num_notes += 1;
		
		ticks.push(tick);
		
		if tick < prev_tick {
			num_manipped_notes += 1;
		}
		
		if deviation.abs() <= 0.09 {
			deviation_sum += deviation;
			num_deviation_notes += 1;
			are_cbs.push(false);
		} else {
			are_cbs.push(true);
		}
		
		if column < 4 {
			score.notes_per_column[column as usize] += 1;
			
			if deviation.abs() > 0.09 {
				score.cbs_per_column[column as usize] += 1;
			}
		}
		
		if deviation.abs() <= 0.0225 {
			mcombo += 1;
		} else {
			if mcombo > score.longest_mcombo {
				score.longest_mcombo = mcombo;
			}
			mcombo = 0;
		}
		
		let deviation_ms_rounded = (deviation * 1000f64).round() as i64;
		let bucket_index = deviation_ms_rounded + OFFSET_BUCKET_RANGE as i64;
		if bucket_index >= 0 && bucket_index < sub_93_offset_buckets.len() as i64 {
			offset_buckets[bucket_index as usize] += 1;
			if wifescore < 0.93 {
				sub_93_offset_buckets[bucket_index as usize] += 1;
			}
		}
		
		prev_tick = tick;
	}
	
	score.num_deviation_notes = num_deviation_notes;
	score.deviation_mean = deviation_sum / num_deviation_notes as f64;
	score.manipulation = num_manipped_notes as f64 / num_notes as f64;
	score.offset_buckets = offset_buckets;
	score.sub_93_offset_buckets = sub_93_offset_buckets;
	
	ticks.sort_unstable(); // need to do this to be able to convert to seconds
	// TODO the deviance is not applied yet. E.g. when the player starts tapping early and ending
	// the combo late, the calculated nps is higher than deserved
	let seconds = timing_info.ticks_to_seconds(&ticks);
	for (is_cb, pairs) in &seconds.iter().zip(are_cbs).group_by(|(_sec, is_cb)| *is_cb) {
		if is_cb { continue } // we don't wanna have combos of cbs, only combos of actual hits
		
		let (first_last_pair, num_notes) = crate::util::first_and_last_and_count(pairs);
		let combo_duration = match first_last_pair {
			Some(((first_second, _), (last_second, _))) => last_second - first_second,
			None => continue, // it's a 0-note or 1-note "combo"
		};
		
		let combo_duration = combo_duration / rate; // apply rate!
		
		//~ if num_notes < 20 { continue } // a combo at so few notes is kinda wonky. better ignore
		if num_notes < 100 { continue }
		
		// without -1 a simple <tap><1 sec pause><tap> would be counted as 2 NPS cuz those are 2
		// taps in one second
		let num_notes = num_notes - 1;
		
		let nps = num_notes as f64 / combo_duration;
		if nps > score.fastest_combo.nps {
			score.fastest_combo = FastestComboInScoreInfo { length: num_notes, nps };
		}
	}
	
	return Some(score);
}

fn calculate_standard_deviation(offset_buckets: &[u64]) -> f64 {
	/*
	standard deviation is `sqrt(mean(square(values - mean(values)))`
	modified version with weights:
	`sqrt(mean(square(values - mean(values, weights)), weights))`
	or, with the "mean(values, weights)" construction expanded:
	
	sqrt(
		sum(
			weights
			*
			square(
				values
				-
				sum(values * weights) / sum(weights)))
		/
		sum(weights)
	*/
	
	// util function
	let iter_value_weight_pairs = || offset_buckets.iter()
			.enumerate()
			.map(|(i, weight)| ((i - OFFSET_BUCKET_RANGE as usize) as i64, weight));
	
	let mut value_x_weights_sum = 0;
	let mut weights_sum = 0;
	for (value, &weight) in iter_value_weight_pairs() {
		value_x_weights_sum += value * weight as i64;
		weights_sum += weight;
	}
	
	let temp_value = value_x_weights_sum / weights_sum as i64;
	
	let mut temp_sum = 0;
	for (value, &weight) in iter_value_weight_pairs() {
		temp_sum += weight as i64 * (value - temp_value).pow(2);
	}
	
	let standard_deviation = (temp_sum as f64 / weights_sum as f64).sqrt();
	return standard_deviation;
}

#[pymethods]
impl ReplaysAnalysis {
	#[new]
	pub fn create(prefix: &str, scorekeys: Vec<&str>, wifescores: Vec<f64>,
			packs: Vec<&str>, songs: Vec<&str>,
			rates: Vec<f64>,
			songs_root: &str
		) -> Self {
		
		let mut analysis = Self::default();
		analysis.offset_buckets = vec![0; NUM_OFFSET_BUCKETS as usize];
		analysis.sub_93_offset_buckets = vec![0; NUM_OFFSET_BUCKETS as usize];
		
		let timing_info_index = crate::build_timing_info_index(&PathBuf::from(songs_root));
		
		let tuples: Vec<_> = izip!(scorekeys, wifescores, packs, songs, rates).collect();
		let score_analyses: Vec<_> = tuples
				.par_iter()
				.filter_map(|(scorekey, wifescore, pack, song, rate)| {
					let replay_path = prefix.to_string() + scorekey;
					let song_id = crate::SongId { pack: pack.to_string(), song: song.to_string() };
					let timing_info = &timing_info_index.get(&song_id)?;
					let score_option = analyze(&replay_path, *wifescore, &timing_info, *rate);
					return Some((scorekey, score_option));
				})
				.collect();
		
		let mut deviation_mean_sum: f64 = 0.0;
		let mut longest_mcombo: u64 = 0;
		let mut longest_mcombo_scorekey: &str = "<no chart>";
		for (i, (scorekey, score_option)) in score_analyses.iter().enumerate() {
			let score = match score_option { Some(a) => a, None => continue };
			
			analysis.score_indices.push(i as u64);
			analysis.manipulations.push(score.manipulation);
			deviation_mean_sum += score.deviation_mean;
			
			for i in 0..4 {
				analysis.cbs_per_column[i] += score.cbs_per_column[i];
				analysis.notes_per_column[i] += score.notes_per_column[i];
			}
			
			// TODO use zipped iterators to avoid bounds-checking cost
			for i in 0..NUM_OFFSET_BUCKETS as usize {
				analysis.offset_buckets[i] += score.offset_buckets[i];
				analysis.sub_93_offset_buckets[i] += score.sub_93_offset_buckets[i];
			}
			
			if score.longest_mcombo > longest_mcombo {
				longest_mcombo = score.longest_mcombo;
				longest_mcombo_scorekey = scorekey;
			}
			
			if score.fastest_combo.nps > analysis.fastest_combo.nps {
				analysis.fastest_combo = FastestComboInfo {
					nps: score.fastest_combo.nps,
					length: score.fastest_combo.length,
					scorekey: scorekey.to_string(),
				}
			}
		}
		let num_scores = analysis.manipulations.len();
		analysis.deviation_mean = deviation_mean_sum / num_scores as f64;
		analysis.longest_mcombo = (longest_mcombo, longest_mcombo_scorekey.into());
		
		analysis.standard_deviation = calculate_standard_deviation(&analysis.offset_buckets);
		
		return analysis;
	}
} 
