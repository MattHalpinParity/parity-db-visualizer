use clap::Parser;
use plotters::{prelude::*};
use std::{error::Error, io::BufRead, collections::HashMap, path::PathBuf};

const COMMIT_SIZE: usize = 100;

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum ChartType {
    CommitTime,
    QueryTime,
    CommitsPerSecond,
    QueriesPerSecond,
}

#[derive(Debug, Parser)]
pub struct Args {
    #[arg(short, long, required = true, num_args(0..))]
    pub data_path: Option<Vec<PathBuf>>,

    #[arg(short, long, value_enum, default_values_t = [ChartType::CommitsPerSecond, ChartType::QueriesPerSecond], num_args(0..))]
    pub chart_type: Vec<ChartType>,

    #[arg(short, long, default_value_t = false)]
    pub small_image: bool,
}

#[derive(Debug)]
pub struct Params {
    pub stroke_width: u64,
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

    let stroke_width = match args.small_image {
        false => 2,
        true => 1,
    };
    let params = Params { stroke_width: stroke_width };

    let root_area = BitMapBackend::new(output_path.as_path(), image_size).into_drawing_area();

    root_area.fill(&WHITE)?;

    let data = get_stress_test_data(&args);
    
    if let Some(data_value) = data {
        draw_stress_test_data(&root_area, &data_value, &args, &params)?;
    }

    root_area.present().expect("Unable to write result to file");

    println!("Wrote file: {}", output_path.display());

    Ok(())
}

struct SampleSet {
    pub samples : Vec<f64>,
    pub value_min : f64,
    pub value_max : f64,
}

impl SampleSet {
    pub fn new() -> SampleSet {
        SampleSet { samples: Default::default(), value_min: 0.0, value_max: 0.0 }
    }

    pub fn add_sample(&mut self, sample: f64) {
        match self.samples.len() {
            0 => {
                self.value_min = sample;
                self.value_max = sample;
            }
            _ => {
                self.value_min = self.value_min.min(sample);
                self.value_max = self.value_max.max(sample);
            }
        }

        self.samples.push(sample);
    }

    pub fn get_mean(&self) -> f64 {
        self.samples.iter().sum::<f64>() / self.samples.len() as f64
    }
}

struct ValueSet {
    pub num_commits : u64,
    pub commit_time : SampleSet,
    pub query_time : SampleSet,
}

impl ValueSet {
    pub fn new(num_commits: u64) -> ValueSet {
        ValueSet { num_commits: num_commits, commit_time: SampleSet::new(), query_time: SampleSet::new() }
    }

    pub fn add_sample(&mut self, commit_time: f64, query_time: f64) {
        self.commit_time.add_sample(commit_time);
        self.query_time.add_sample(query_time);
    }
}

struct DataSet {
    pub sorted_values : Vec<ValueSet>,
}

impl DataSet {
    pub fn add_sample(&mut self, commits: u64, commit_time: f64, query_time: f64) {
        match self.sorted_values.binary_search_by(|probe| probe.num_commits.cmp(&commits)) {
            Ok(val) => self.sorted_values[val].add_sample(commit_time, query_time),
            Err(val) => {
                let mut valueset = ValueSet::new(commits);
                valueset.add_sample(commit_time, query_time);
                self.sorted_values.insert(val, valueset);
            },
        }
    }
}

struct StressTestData {
    pub datasets : HashMap<String, DataSet>,
    pub max_commits: u64,
    pub max_commit_time: f64,
    pub max_query_time: f64,
    pub max_commits_per_second: f64,
    pub max_queries_per_second: f64,
}

impl StressTestData {
    pub fn new() -> StressTestData {
        StressTestData { datasets: Default::default(), max_commits: 0, max_commit_time: 0.0f64, max_query_time: 0.0f64, max_commits_per_second: 0.0f64, max_queries_per_second: 0.0f64 }
    }

    pub fn add_sample(&mut self, name: String, commits: u64, commit_time: f64, query_time: f64) {
        self.max_commits = std::cmp::max(self.max_commits, commits);
        self.max_commit_time = self.max_commit_time.max(commit_time);
        self.max_query_time = self.max_query_time.max(query_time);
        self.max_commits_per_second = self.max_commits_per_second.max(commits as f64 / commit_time);
        self.max_queries_per_second = self.max_queries_per_second.max((commits * COMMIT_SIZE as u64) as f64 / query_time);

        match self.datasets.entry(name) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().add_sample(commits, commit_time, query_time);
            },
            std::collections::hash_map::Entry::Vacant(entry) => {
                let mut dataset = DataSet { sorted_values: Default::default() };
                dataset.add_sample(commits, commit_time, query_time);
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
            let elements = line.split(',').collect::<Vec<_>>();
            let pruning: bool = elements[2].parse().unwrap();
            let mut name = elements[0].to_string();
            if pruning { name = name + " (Pruning)"; }
            data.add_sample(name, elements[1].parse().unwrap(), elements[3].parse().unwrap(), elements[4].parse().unwrap());
        }
    }

    Some(data)
}

fn draw_stress_test_data<DB: DrawingBackend>(b: &DrawingArea<DB, plotters::coord::Shift>, data: &StressTestData, args: &Args, params: &Params) -> Result<(), Box<dyn Error>> where DB::ErrorType: 'static {

    let mut colours : Vec<RGBColor> = Default::default();
    colours.push(full_palette::LIGHTBLUE);
    colours.push(full_palette::GREEN);
    colours.push(full_palette::YELLOW);
    colours.push(full_palette::RED);
    colours.push(full_palette::BLACK);
    colours.push(full_palette::ORANGE);

    let mut datasets_presort = Vec::new();
    for entry in &data.datasets {
        datasets_presort.push((entry.0, entry.1));
    }

    datasets_presort.sort_by(|a, b| a.0.cmp(b.0));

    let mut datasets = Vec::new();
    let mut colour_index = 0;
    for entry in datasets_presort {
        datasets.push((entry.0, entry.1, colours[colour_index].clone().stroke_width(params.stroke_width as u32), colours[colour_index].clone().stroke_width(params.stroke_width as u32 * 2)));
        colour_index = (colour_index + 1) % colours.len();
    }

    {
        let mut areas = Vec::new();
        let area_values = match args.chart_type.len() {
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

        let chart_types = args.chart_type.clone();

        for i in 0..std::cmp::min(areas.len(), chart_types.len()) {
            let area = areas[i];
            let chart_type = &chart_types[i];

            let title = match chart_type {
                ChartType::CommitTime => "Commit Time",
                ChartType::QueryTime => "Query Time",
                ChartType::CommitsPerSecond => "Commits per Second",
                ChartType::QueriesPerSecond => "Queries per Second",
            };

            let max_y = match chart_type {
                ChartType::CommitTime => data.max_commit_time,
                ChartType::QueryTime => data.max_query_time,
                ChartType::CommitsPerSecond => data.max_commits_per_second,
                ChartType::QueriesPerSecond => data.max_queries_per_second,
            };

            let pixel_height = (area.get_pixel_range().1.end - area.get_pixel_range().1.start) as f64;

            let mut cc = ChartBuilder::on(&area)
                .x_label_area_size((5).percent_height())
                .y_label_area_size((6).percent_height())
                .margin((2).percent_height())
                .margin_right((5).percent_height())
                .caption(title, ("sans-serif", (5).percent_height()))
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
                let mut points: Vec<(f64, f64)> = Default::default();
                let mut errorbars: Vec<(f64, f64, f64, f64)> = Default::default();
                for value in &entry.1.sorted_values {
                    let x = value.num_commits as f64;

                    let value_data = match chart_type {
                        ChartType::CommitTime => (x, value.commit_time.value_min, value.commit_time.get_mean(), value.commit_time.value_max),
                        ChartType::QueryTime => (x, value.query_time.value_min, value.query_time.get_mean(), value.query_time.value_max),
                        ChartType::CommitsPerSecond => (x, x / value.commit_time.value_max, x / value.commit_time.get_mean(), x / value.commit_time.value_min),
                        ChartType::QueriesPerSecond => (x, (x * COMMIT_SIZE as f64) / value.query_time.value_max, (x * COMMIT_SIZE as f64) / value.query_time.get_mean(), (x * COMMIT_SIZE as f64) / value.query_time.value_min),
                    };

                    points.push((value_data.0, value_data.2));
                    errorbars.push(value_data);
                }

                cc.draw_series(LineSeries::new(points, entry.3))?
                    .label(entry.0)
                    .legend(|(x, y)| PathElement::new(vec![(x, y), (x + (pixel_height * 0.03) as i32, y)], entry.3));

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

            cc.configure_series_labels().legend_area_size((5).percent_height()).margin((1).percent_height()).border_style(&BLACK).label_font(("sans-serif", (2).percent_height())).draw()?;
        }
    }

    Ok(())
}