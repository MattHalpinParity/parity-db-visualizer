use super::*;

pub trait FilterSet {
    fn passes_filters(&self, parameters: &BTreeMap<String, ParameterValue>) -> bool;
    fn display_text(&self) -> String;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Comparison {
    Less,
    LessEqual,
    Equal,
    GreaterEqual,
    Greater,
}

// In order that they should be matched with text.
static COMPARISONS: [Comparison; 5] = [Comparison::Equal, Comparison::LessEqual, Comparison::GreaterEqual, Comparison::Less, Comparison::Greater];

impl Comparison {
    pub fn get_text(&self) -> String {
        match self {
            Comparison::Less => return "<".to_string(),
            Comparison::LessEqual => return "<=".to_string(),
            Comparison::Equal => return "==".to_string(),
            Comparison::GreaterEqual => return ">=".to_string(),
            Comparison::Greater => return ">".to_string(),
        }
    }
}

#[derive(Debug)]
pub enum ParameterFilter {
    // Bool filters are assumed to be Equal. Just stores the value to compare against.
    Bool(String, bool),
    // Int filters store the reference value and the Comparison to use between the value and reference value.
    Int(String, Comparison, u64),
}

impl ParameterFilter {
    pub fn name(&self) -> &String {
        match self {
            ParameterFilter::Bool(name, _) => {
                return name
            },
            ParameterFilter::Int(name, _, _) => {
                return name
            }
        }
    }
}

#[derive(Debug)]
pub struct ParameterFilterSet {
    filters: Vec<ParameterFilter>,
}

impl ParameterFilterSet {
    pub fn new(filter_text: &String) -> ParameterFilterSet {
        let mut comparisons: Vec<(String, Comparison, String)> = Default::default();

        let pairs = filter_text.split(',').collect::<Vec<_>>();
        for m in pairs.iter() {
            for c in &COMPARISONS {
                if let Some(pos) = m.find(&c.get_text()) {
                    let first = &m[0..pos].trim();
                    let second = &m[pos + c.get_text().len()..].trim();
                    comparisons.push((first.to_string(), c.clone(), second.to_string()));
                    break
                }
            }
        }

        let mut filters: Vec<ParameterFilter> = Default::default();

        for (name, comparison, value_text) in &comparisons {
            if let Ok(v) = value_text.parse::<bool>() {
                assert_eq!(*comparison, Comparison::Equal);
                filters.push(ParameterFilter::Bool(name.clone(), v));
            }
            else if let Ok(v) = value_text.parse::<u64>() {
                filters.push(ParameterFilter::Int(name.clone(), comparison.clone(), v));
            }
        }

        filters.sort_by(|a, b| a.name().cmp(b.name()));

        ParameterFilterSet { filters: filters }
    }
}

impl FilterSet for ParameterFilterSet {
    fn passes_filters(&self, parameters: &BTreeMap<String, ParameterValue>) -> bool {
        let mut passes = true;
        for filter in &self.filters {
            match filter {
                ParameterFilter::Bool(filter_name, filter_value) => {
                    if let Some(param) = parameters.get(filter_name) {
                        match param {
                            ParameterValue::Bool(param_value) => {
                                if param_value != filter_value {
                                    passes = false;
                                }
                            },
                            _ => {
                            },
                        }
                    };
                },
                ParameterFilter::Int(filter_name, filter_comp, filter_value) => {
                    if let Some(param) = parameters.get(filter_name) {
                        match param {
                            ParameterValue::Int(param_value) => {
                                match filter_comp {
                                    Comparison::Less => {
                                        if param_value >= filter_value {
                                            passes = false;
                                        }
                                    },
                                    Comparison::LessEqual => {
                                        if param_value > filter_value {
                                            passes = false;
                                        }
                                    },
                                    Comparison::Equal => {
                                        if param_value != filter_value {
                                            passes = false;
                                        }
                                    },
                                    Comparison::GreaterEqual => {
                                        if param_value < filter_value {
                                            passes = false;
                                        }
                                    },
                                    Comparison::Greater => {
                                        if param_value <= filter_value {
                                            passes = false;
                                        }
                                    },
                                }
                            },
                            _ => {
                            },
                        }
                    };
                }
            }
        }
        passes
    }

    fn display_text(&self) -> String {
        let mut text = String::new();

        let mut prev_filter = false;
        for filter in &self.filters {
            if prev_filter {
                text += ", ";
            }
            match filter {
                ParameterFilter::Bool(filter_name, filter_value) => {
                    text += &format!("{}={}", filter_name, filter_value);
                },
                ParameterFilter::Int(filter_name, filter_comp, filter_value) => {
                    text += &format!("{}{}{}", filter_name, filter_comp.get_text(), filter_value);
                },
            }
            prev_filter = true;
        }

        text
    }
}