use clap::Parser;
use plotters::{prelude::*};
use std::{error::Error, io::BufRead, collections::{HashMap, HashSet, BTreeMap}, path::PathBuf, fmt::Debug};

mod filter;
use filter::{FilterSet, ParameterFilterSet};

#[derive(Debug, Clone, PartialEq)]
pub enum ParameterValue {
    Bool(bool),
    Int(u64),
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ChartType {
    CommitTime,
    CommitsPerSecond,
    QueriesPerSecond,
}

impl ChartType {
    pub fn get_from_string(text: &String) -> Option<ChartType> {
        match text.as_str() {
            "commit-time" => Some(ChartType::CommitTime),
            "commits-per-second" => Some(ChartType::CommitsPerSecond),
            "queries-per-second" => Some(ChartType::QueriesPerSecond),
            _ => None,
        }
    }
}

#[derive(Debug, Parser)]
pub struct Args {
    #[arg(short, long, required = true, num_args(0..))]
    pub data_path: Option<Vec<PathBuf>>,

    #[arg(short, long, value_enum, default_values_t = [ChartType::CommitsPerSecond, ChartType::QueriesPerSecond], num_args(0..))]
    pub chart_type: Vec<ChartType>,

    #[arg(short, long, default_values_t = ["progressive==true, readers==0".to_string(), "progressive==true, readers>0".to_string()], num_args(0..))]
    pub chart_filter: Vec<String>,

    #[arg(short, long, default_value_t = false)]
    pub small_image: bool,
}

#[derive(Debug)]
pub struct ChartSpec {
    pub chart_type: ChartType,
    pub filters: ParameterFilterSet,
}

#[derive(Debug)]
pub struct Params {
    pub stroke_width: u64,
    pub chart_specs: Vec<ChartSpec>,
}

pub fn run_visualizer() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let mut output_path = std::env::current_dir().expect("Cannot resolve current dir");
    output_path.push("visualizer_output");
    std::fs::create_dir_all(&output_path).expect("Failed to create visualizer_output directory");
    output_path.push("stress_test_charts.png");

    let chart_size_scale = match args.small_image { 
        false => 2,
        true => 1,
    };

    let chart_width = 1080 * chart_size_scale;
    let chart_height = 1080 * chart_size_scale;

    let image_size = match args.chart_type.len() {
        0 => {(chart_width, chart_height)},
        1 => {(chart_width, chart_height)},
        2 => {(chart_width * 2, chart_height)},
        3 => {(chart_width * 3, chart_height)},
        _ => {(chart_width * 2, chart_height * 2)},
    };

    // Params
    let params = {
        let stroke_width = match args.small_image {
            false => 2,
            true => 1,
        };

        let mut chart_specs: Vec<ChartSpec> = Default::default();
        for i in 0..args.chart_type.len() {
            let chart_type = args.chart_type[i].clone();

            let filter_text = if i < args.chart_filter.len() {
                args.chart_filter[i].clone()
            } else {
                "".to_string()
            };

            let filters = ParameterFilterSet::new(&filter_text);

            let chart_spec = ChartSpec {
                chart_type: chart_type,
                filters: filters,
            };

            chart_specs.push(chart_spec);
        }

        Params { stroke_width: stroke_width, chart_specs: chart_specs }
    };

    let root_area = BitMapBackend::new(output_path.as_path(), image_size).into_drawing_area();

    root_area.fill(&WHITE)?;

    let data = get_stress_test_data(&args);
    
    if let Some(data_value) = data {
        draw_stress_test_data(&root_area, &data_value, &params)?;
    }

    root_area.present().expect("Unable to write result to file");

    println!("Wrote file: {}", output_path.display());

    Ok(())
}

struct RunningStatistics {
    pub num: u64,
    pub old_m: f64,
    pub new_m: f64,
    pub old_s: f64,
    pub new_s: f64,
}

impl RunningStatistics {
    pub fn new() -> RunningStatistics {
        RunningStatistics { num: 0, old_m: 0.0, new_m: 0.0, old_s: 0.0, new_s: 0.0 }
    }

    pub fn add_sample(&mut self, sample: f64) {
        self.num += 1;

        if self.num == 1 {
            self.old_m = sample;
            self.new_m = sample;
            self.old_s = 0.0;
        }
        else {
            self.new_m = self.old_m + ((sample - self.old_m) / self.num as f64);
            self.new_s = self.old_s + ((sample - self.old_m) * (sample - self.new_m));

            self.old_m = self.new_m;
            self.old_s = self.new_s;
        }
    }

    pub fn mean(&self) -> f64 {
        if self.num > 0 {
            return self.new_m
        }
        0.0
    }

    pub fn variance(&self) -> f64 {
        if self.num > 1 {
            return self.new_s / ((self.num - 1) as f64)
        }
        0.0
    }
}

struct SampleSet {
    pub samples : Vec<f64>,
    pub value_min : f64,
    pub value_max : f64,
    pub statistics : RunningStatistics,
}

impl SampleSet {
    pub fn new() -> SampleSet {
        SampleSet { samples: Default::default(), value_min: 0.0, value_max: 0.0, statistics: RunningStatistics::new() }
    }

    pub fn add_sample(&mut self, sample: f64) {
        match self.samples.len() {
            0 => {
                self.value_min = sample;
                self.value_max = sample;
            },
            _ => {
                self.value_min = self.value_min.min(sample);
                self.value_max = self.value_max.max(sample);
            },
        }

        self.samples.push(sample);

        self.statistics.add_sample(sample);
    }

    pub fn get_mean(&self) -> f64 {
        self.statistics.mean()
    }

    fn get_half_range(&self) -> f64 {
        //self.statistics.variance() * 4.0
        f64::sqrt(self.statistics.variance()) * 2.0
    }

    pub fn get_range_start(&self) -> f64 {
        self.statistics.mean() - self.get_half_range()
    }

    pub fn get_range_end(&self) -> f64 {
        self.statistics.mean() + self.get_half_range()
    }
}

struct ValueSet {
    pub num_commits : u64,
    pub commit_time : SampleSet,
    pub commits_per_second : SampleSet,
    pub queries_per_second : SampleSet,
}

impl ValueSet {
    pub fn new(num_commits: u64) -> ValueSet {
        ValueSet { num_commits: num_commits, commit_time: SampleSet::new(), commits_per_second: SampleSet::new(), queries_per_second: SampleSet::new() }
    }

    pub fn add_sample(&mut self, commit_time: f64, commits_per_second: f64, queries_per_second: f64) {
        self.commit_time.add_sample(commit_time);
        self.commits_per_second.add_sample(commits_per_second);
        self.queries_per_second.add_sample(queries_per_second);
    }
}

struct DataSet {
    pub base_name : String,
    pub parameters: BTreeMap<String, ParameterValue>,

    pub sorted_values : Vec<ValueSet>,

    pub max_commits: u64,
    pub max_commit_time: f64,
    pub max_commits_per_second: f64,
    pub max_queries_per_second: f64,
}

impl DataSet {
    pub fn new(base_name: String, parameters: BTreeMap<String, ParameterValue>) -> DataSet {
        DataSet {
            base_name: base_name,
            parameters: parameters,
            sorted_values: Default::default(), 
            max_commits: 0, max_commit_time: 0.0f64, max_commits_per_second: 0.0f64, max_queries_per_second: 0.0f64 }
    }

    pub fn add_sample(&mut self, commits: u64, commit_time: f64, commits_per_second: f64, queries_per_second: f64) {
        self.max_commits = std::cmp::max(self.max_commits, commits);
        self.max_commit_time = self.max_commit_time.max(commit_time);
        self.max_commits_per_second = self.max_commits_per_second.max(commits_per_second);
        self.max_queries_per_second = self.max_queries_per_second.max(queries_per_second);

        match self.sorted_values.binary_search_by(|probe| probe.num_commits.cmp(&commits)) {
            Ok(val) => self.sorted_values[val].add_sample(commit_time, commits_per_second, queries_per_second),
            Err(val) => {
                let mut valueset = ValueSet::new(commits);
                valueset.add_sample(commit_time, commits_per_second, queries_per_second);
                self.sorted_values.insert(val, valueset);
            },
        }
    }

    pub fn get_name(base_name: String, parameters: &BTreeMap<String, ParameterValue>) -> String {
        let mut suffix = String::new();

        let mut prev_param = false;
        for (name, value) in parameters {
            if prev_param {
                suffix += " ";
            }

            match value {
                ParameterValue::Bool(v) => {
                    if *v {
                        suffix += name;
                        prev_param = true;
                    }
                },
                ParameterValue::Int(v) => {
                    suffix += &format!("{}={}", name, *v);
                    prev_param = true;
                },
            }
        }
        if suffix.len() > 0 {
            suffix = " (".to_string() + &suffix + ")";
        }

        base_name.clone() + &suffix
    }

    pub fn get_name_including(base_name: String, parameters: &BTreeMap<String, ParameterValue>, include_parameters: &HashSet<String>) -> String {
        let mut suffix = String::new();

        let mut prev_param = false;
        for (name, value) in parameters {
            if include_parameters.contains(name) {
                if prev_param {
                    suffix += " ";
                }
    
                match value {
                    ParameterValue::Bool(v) => {
                        if *v {
                            suffix += name;
                            prev_param = true;
                        }
                    },
                    ParameterValue::Int(v) => {
                        suffix += &format!("{}={}", name, *v);
                        prev_param = true;
                    },
                }
            }
        }
        if suffix.len() > 0 {
            suffix = " (".to_string() + &suffix + ")";
        }

        base_name.clone() + &suffix
    }

    pub fn passes_filters(&self, filters: &impl FilterSet) -> bool {
        filters.passes_filters(&self.parameters)
    }
}

struct StressTestData {
    pub datasets : HashMap<String, DataSet>,

    pub max_commits: u64,
    pub max_commit_time: f64,
    pub max_commits_per_second: f64,
    pub max_queries_per_second: f64,
}

impl StressTestData {
    pub fn new() -> StressTestData {
        StressTestData { datasets: Default::default(), max_commits: 0, max_commit_time: 0.0f64, max_commits_per_second: 0.0f64, max_queries_per_second: 0.0f64 }
    }

    pub fn add_sample(&mut self, base_name: String, parameters: BTreeMap<String, ParameterValue>, commits: u64, commit_time: f64, commits_per_second: f64, queries_per_second: f64) {
        self.max_commits = std::cmp::max(self.max_commits, commits);
        self.max_commit_time = self.max_commit_time.max(commit_time);
        self.max_commits_per_second = self.max_commits_per_second.max(commits_per_second);
        self.max_queries_per_second = self.max_queries_per_second.max(queries_per_second);

        let full_name = DataSet::get_name(base_name.clone(), &parameters);

        match self.datasets.entry(full_name) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().add_sample(commits, commit_time, commits_per_second, queries_per_second);
            },
            std::collections::hash_map::Entry::Vacant(entry) => {
                let mut dataset = DataSet::new(base_name, parameters);
                dataset.add_sample(commits, commit_time, commits_per_second, queries_per_second);
                entry.insert(dataset);
            },
        }
    }
}

fn get_stress_test_data(args: &Args) -> Option<StressTestData> {
    let paths = args.data_path.clone()?;

    let mut data = StressTestData::new();

    for path in paths {
        println!("Reading data file: {}", path.display());

        let file = std::fs::OpenOptions::new()
            .read(true)
            .open(path.as_path()).expect(format!("Failed to open data file {}", path.display()).as_str());

        let reader = std::io::BufReader::new(file);

        // First line is column names, so skip.
        for line in reader.lines().skip(1).map(|l| l.unwrap()) {
            let mut elements = line.split(',');

            let base_name = elements.next().unwrap().to_string();

            let archive: bool = elements.next().unwrap().parse().unwrap();
            let compress: bool = elements.next().unwrap().parse().unwrap();
            let ordered: bool = elements.next().unwrap().parse().unwrap();
            let uniform: bool = elements.next().unwrap().parse().unwrap();
            let num_readers: u64 = elements.next().unwrap().parse().unwrap();
            let num_writers: u64 = elements.next().unwrap().parse().unwrap();
            let writer_commits_per_sleep: u64 = elements.next().unwrap().parse().unwrap();
            let writer_sleep_time: u64 = elements.next().unwrap().parse().unwrap();
            let commits_per_timing_sample: u64 = elements.next().unwrap().parse().unwrap();
            let progressive: bool = elements.next().unwrap().parse().unwrap();

            let total_commits = elements.next().unwrap().parse().unwrap();
            let total_commit_time = elements.next().unwrap().parse().unwrap();

            let commits: u64 = elements.next().unwrap().parse().unwrap();
            let commit_time: f64 = elements.next().unwrap().parse().unwrap();

            let queries: u64 = elements.next().unwrap().parse().unwrap();
            let query_time: f64 = elements.next().unwrap().parse().unwrap();

            let commits_per_second = commits as f64 / commit_time;
            let queries_per_second = queries as f64 / query_time;

            let mut parameters: BTreeMap<String, ParameterValue> = Default::default();
            parameters.insert("archive".to_string(), ParameterValue::Bool(archive));
            parameters.insert("compress".to_string(), ParameterValue::Bool(compress));
            parameters.insert("ordered".to_string(), ParameterValue::Bool(ordered));
            parameters.insert("uniform".to_string(), ParameterValue::Bool(uniform));
            parameters.insert("readers".to_string(), ParameterValue::Int(num_readers));
            parameters.insert("writers".to_string(), ParameterValue::Int(num_writers));
            parameters.insert("writer-commits-per-sleep".to_string(), ParameterValue::Int(writer_commits_per_sleep));
            parameters.insert("writer-sleep-time".to_string(), ParameterValue::Int(writer_sleep_time));
            parameters.insert("commits-per-timing-sample".to_string(), ParameterValue::Int(commits_per_timing_sample));
            parameters.insert("progressive".to_string(), ParameterValue::Bool(progressive));
    
            data.add_sample(base_name, parameters, total_commits, total_commit_time, commits_per_second, queries_per_second);
        }
    }

    Some(data)
}

fn draw_stress_test_data<DB: DrawingBackend>(b: &DrawingArea<DB, plotters::coord::Shift>, data: &StressTestData, params: &Params) -> Result<(), Box<dyn Error>> where DB::ErrorType: 'static {

    let mut colours : Vec<RGBColor> = Default::default();
    colours.push(full_palette::LIGHTBLUE);
    colours.push(full_palette::GREEN);
    colours.push(full_palette::YELLOW);
    colours.push(full_palette::RED);
    colours.push(full_palette::BLACK);
    colours.push(full_palette::BROWN_400);
    colours.push(full_palette::PINK);
    colours.push(full_palette::ORANGE);
    colours.push(full_palette::GREY);

    let mut datasets_presort = Vec::new();
    for entry in &data.datasets {
        datasets_presort.push((entry.0, entry.1));
    }

    datasets_presort.sort_by(|a, b| a.0.cmp(b.0));

    let mut datasets = Vec::new();
    let mut colour_index = 0;
    for entry in datasets_presort {
        datasets.push((entry.0, entry.1, colours[colour_index].clone().stroke_width(params.stroke_width as u32), colours[colour_index].clone().stroke_width(params.stroke_width as u32 * 2), colours[colour_index].mix(0.75)));
        colour_index = (colour_index + 1) % colours.len();
    }

    {
        let mut areas = Vec::new();
        let area_values = match params.chart_specs.len() {
            0 => {
                Vec::new()
            }
            1 => {
                areas.push(b);
                Vec::new()
            }
            2 => {
                b.split_evenly((1, 2))
            }
            3 => {
                b.split_evenly((1, 3))
            }
            _ => {
                b.split_evenly((2, 2))
            }
        };
        for area in area_values.iter() {
            areas.push(area);
        }

        let chart_types: Vec<ChartType> = params.chart_specs.iter().map(|x| x.chart_type.clone()).collect();

        for i in 0..std::cmp::min(areas.len(), chart_types.len()) {
            let area = areas[i];
            let chart_type = &chart_types[i];

            let mut title = match chart_type {
                ChartType::CommitTime => "Commit Time",
                ChartType::CommitsPerSecond => "Commits per Second",
                ChartType::QueriesPerSecond => "Queries per Second",
            }.to_string();

            let filter_text = params.chart_specs[i].filters.display_text();
            if filter_text.len() > 0 {
                title += " (";
                title += &filter_text;
                title += ")";
            }

            let mut max_y: f64 = 0.0;
            let mut first_dataset: Option<&DataSet> = None;
            let mut include_parameters: HashSet<String> = Default::default();
            for entry in &datasets {
                let passed_filters = entry.1.passes_filters(&params.chart_specs[i].filters);
                if passed_filters {
                    let dataset_max_y = match chart_type {
                        ChartType::CommitTime => entry.1.max_commit_time,
                        ChartType::CommitsPerSecond => entry.1.max_commits_per_second,
                        ChartType::QueriesPerSecond => entry.1.max_queries_per_second,
                    };
                    max_y = max_y.max(dataset_max_y as f64);

                    match first_dataset {
                        Some(dataset) => {
                            let other = entry.1;
                            for (name, value) in &dataset.parameters {
                                match other.parameters.get(name) {
                                    Some(other_value) => {
                                        if other_value != value {
                                            include_parameters.insert(name.clone());
                                        }
                                    },
                                    None => {
                                        include_parameters.insert(name.clone());
                                    },
                                }
                            }
                            for (name, _) in &other.parameters {
                                match dataset.parameters.get(name) {
                                    Some(_) => {},
                                    None => {
                                        include_parameters.insert(name.clone());
                                    },
                                }
                            }
                        },
                        None => {
                            first_dataset = Some(entry.1);
                        }
                    }
                }
            }

            let pixel_height = (area.get_pixel_range().1.end - area.get_pixel_range().1.start) as f64;

            let mut cc = ChartBuilder::on(&area)
                .x_label_area_size((5).percent_height())
                .y_label_area_size((6).percent_height())
                .margin((2).percent_height())
                .margin_right((5).percent_height())
                .caption(title, ("sans-serif", (3).percent_height()))
                .build_cartesian_2d(0.0f64..data.max_commits as f64, 0.0f64..max_y)?;

            cc.configure_mesh()
                .x_desc("Commits")
                .x_labels(10)
                .y_labels(8)
                .label_style(("sans-serif", (2).percent_height()))
                .x_label_formatter(&|v| format!("{:.0}", v))
                .draw()?;

            let pixel_range = cc.plotting_area().get_pixel_range();
            let coord_to_pixel_x = (pixel_range.0.end - pixel_range.0.start) as f64 / ((cc.x_range().end - cc.x_range().start) as f64);
            let coord_to_pixel_y = (pixel_range.1.end - pixel_range.1.start) as f64 / ((cc.y_range().end - cc.y_range().start) as f64);

            let pixel_offset = |origin: (f64, f64), pos: (f64, f64), offset: (i32, i32)| -> (i32, i32) {
                (((pos.0 - origin.0) * coord_to_pixel_x) as i32 + offset.0, ((pos.1 - origin.1) * -coord_to_pixel_y) as i32 + offset.1)
            };

            let marker_size = (pixel_height * 0.0025) as i32;
            let errorbar_size = (pixel_height * 0.004) as i32;

            for entry in &datasets {
                let passed_filters = entry.1.passes_filters(&params.chart_specs[i].filters);
                if passed_filters {
                    let mut points: Vec<(f64, f64)> = Default::default();
                    let mut points_neg: Vec<(f64, f64)> = Default::default();
                    let mut points_pos: Vec<(f64, f64)> = Default::default();
                    let mut errorbars: Vec<(f64, f64, f64, f64)> = Default::default();
                    for value in &entry.1.sorted_values {
                        let x = value.num_commits as f64;

                        let value_data = match chart_type {
                            ChartType::CommitTime => (x, value.commit_time.value_min, value.commit_time.get_range_start(), value.commit_time.get_mean(), value.commit_time.get_range_end(), value.commit_time.value_max),
                            ChartType::CommitsPerSecond => (x, value.commits_per_second.value_min, value.commits_per_second.get_range_start(), value.commits_per_second.get_mean(), value.commits_per_second.get_range_end(), value.commits_per_second.value_max),
                            ChartType::QueriesPerSecond => (x, value.queries_per_second.value_min, value.queries_per_second.get_range_start(), value.queries_per_second.get_mean(), value.queries_per_second.get_range_end(), value.queries_per_second.value_max),
                        };

                        points.push((value_data.0, value_data.3));
                        points_neg.push((value_data.0, value_data.2));
                        points_pos.push((value_data.0, value_data.4));
                        errorbars.push((value_data.0, value_data.1, value_data.3, value_data.5));
                    }

                    let display_name = DataSet::get_name_including(entry.1.base_name.clone(), &entry.1.parameters, &include_parameters);

                    cc.draw_series(LineSeries::new(points, entry.3))?
                        .label(display_name)
                        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + (pixel_height * 0.03) as i32, y)], entry.3));

                    //cc.draw_series(LineSeries::new(points_neg, entry.4))?;
                    //cc.draw_series(LineSeries::new(points_pos, entry.4))?;

                    cc.draw_series(errorbars.iter().map(|(x, min, mean, _)| {
                        EmptyElement::at((*x, *min))
                        + Circle::new(pixel_offset((*x, *min), (*x, *mean), (0, 0)), marker_size, entry.2.filled())
                    }))?;

                    cc.draw_series(errorbars.iter().skip_while(|(_, min, _, max)| { max <= min }).map(|(x, min, _, max)| {
                        EmptyElement::at((*x, *min))
                        + PathElement::new(vec![(0, 0), pixel_offset((*x, *min), (*x, *max), (0, 0))], entry.2)
                        + PathElement::new(vec![(-errorbar_size, 0), (errorbar_size, 0)], entry.2)
                        + PathElement::new(vec![pixel_offset((*x, *min), (*x, *max), (-errorbar_size, 0)), pixel_offset((*x, *min), (*x, *max), (errorbar_size, 0))], entry.2)
                    }))?;
                }
            }

            cc.configure_series_labels().legend_area_size((5).percent_height()).margin((1).percent_height()).border_style(&BLACK).label_font(("sans-serif", (2).percent_height())).draw()?;
        }
    }

    Ok(())
}