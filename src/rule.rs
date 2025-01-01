use crate::configuration::{Pattern, URL};
use crate::git::{merge_base, FileStatus};
use crate::webhook::{perform_request, HookError, WebhookResult};
use nonempty::NonEmpty;
use serde::Deserialize;
use serde_with::{serde_as, DurationMilliSeconds};
use std::fmt::Display;
use std::time::Duration;
use regex::Regex;
use webbed_hook_core::webhook::{Value, WebhookResponse};
use crate::{Change, GitData};

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct WebhookRule {
    pub url: URL,
    pub config: Option<Value>,
    #[serde_as(as = "Option<DurationMilliSeconds<u64>>")]
    pub request_timeout: Option<Duration>,
    #[serde_as(as = "Option<DurationMilliSeconds<u64>>")]
    pub connect_timeout: Option<Duration>,
    pub greeting_messages: Option<NonEmpty<String>>,
}

pub struct RuleContext<'a> {
    pub default_branch: &'a str,
    pub push_options: &'a [String],
    pub change: &'a Change,
}

#[derive(Debug, Deserialize)]
pub enum Condition {
    RefIs {
        name: String,
    },
    RefMatches {
        pattern: Pattern
    },
    AnyCommitMessageMatches {
        pattern: Pattern,
        accept_removes: Option<bool>,
    },
    ModifiedFileMatches {
        pattern: Pattern,
        accept_removes: Option<bool>,
    },
    AddedFileMatches {
        pattern: Pattern,
        accept_removes: Option<bool>,
    },
    RemovedFileMatches {
        pattern: Pattern,
        accept_removes: Option<bool>,
    },
    DerivedFromDefaultBranch {
        accept_removes: Option<bool>,
    },
    DerivedFromBranch {
        accept_removes: Option<bool>,
        name: String,
    },
    LinearHistory,
    RefAdd,
    RefRemove,
    RefUpdate,
    And {
        conditions: Box<NonEmpty<Condition>>,
    },
    Or {
        conditions: Box<NonEmpty<Condition>>,
    },
    Xor {
        conditions: Box<NonEmpty<Condition>>,
    },
    Not {
        condition: Box<Condition>,
    },
    True,
    False,
    BypassRequested {
        option: String,
    },
    Rule {
        rule: Box<Rule>,
    },
}

#[derive(Debug)]
pub enum ConditionError {
    RuleError(Box<RuleError>),
}

impl Display for ConditionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConditionError::RuleError(err) => err.fmt(f),
        }
    }
}

fn is_derived_from(ref_a: &str, change: &Change, accept_removes: &Option<bool>) -> Result<bool, ConditionError> {
    let ref_b = match change {
        Change::UpdateRef { new_commit, .. } => new_commit,
        Change::AddRef { commit, .. } => commit,
        Change::RemoveRef { .. } => return Ok(accept_removes.unwrap_or(false)),
    };
    Ok(merge_base(ref_a, ref_b).is_some())
}

fn any_file_matches<T: Fn(&FileStatus) -> bool>(context: &RuleContext, accept_removes: &Option<bool>, filter: T, pattern: &Regex) -> Result<bool, ConditionError> {
    let file_status: &Vec<(FileStatus, String)> = match context.change {
        Change::AddRef { git_data: GitData { file_status, .. }, .. } => file_status,
        Change::UpdateRef { git_data: GitData { file_status, .. }, .. } => file_status,
        Change::RemoveRef { .. } => return Ok(accept_removes.unwrap_or(true)),
    };
    
    Ok(file_status.iter().any(|(status, name)| {
        filter(status) && pattern.is_match(name.as_str())
    }))
}

impl Condition {
    pub fn evaluate(&self, context: &RuleContext) -> Result<bool, ConditionError> {
        match self {
            Condition::RefIs { name } => {
                Ok(context.change.ref_name() == name.as_str())
            }
            Condition::RefMatches { pattern: Pattern(pattern) } => {
                Ok(pattern.is_match(context.change.ref_name()))
            }
            Condition::AnyCommitMessageMatches { pattern: Pattern(pattern), accept_removes } => {
                let log = match context.change {
                    Change::UpdateRef { git_data: GitData { log, .. }, .. } => log,
                    Change::AddRef { .. } => &vec![],
                    Change::RemoveRef { .. } => return Ok(accept_removes.unwrap_or(true)),
                };
                Ok(log.iter().any(|e| pattern.is_match(e.message.as_str())))
            }
            Condition::ModifiedFileMatches { pattern: Pattern(pattern), accept_removes } => {
                any_file_matches(context, accept_removes, |s| s == &FileStatus::Modified || s == &FileStatus::Renamed, pattern)
            }
            Condition::AddedFileMatches { pattern: Pattern(pattern), accept_removes } => {
                any_file_matches(context, accept_removes, |s| s == &FileStatus::Added, pattern)
            }
            Condition::RemovedFileMatches { pattern: Pattern(pattern), accept_removes } => {
                any_file_matches(context, accept_removes, |s| s == &FileStatus::Deleted, pattern)
            }
            Condition::DerivedFromDefaultBranch { accept_removes } => {
                is_derived_from(context.default_branch, context.change, accept_removes)
            }
            Condition::DerivedFromBranch { name, accept_removes } => {
                is_derived_from(name, context.change, accept_removes)
            }
            Condition::BypassRequested { option } => {
                Ok(context.push_options.contains(option))
            }
            Condition::And { conditions} => {
                for condition in conditions.iter() {
                    if !condition.evaluate(context)? {
                        return Ok(false)
                    }
                }
                Ok(true)
            }
            Condition::Or { conditions} => {
                for condition in conditions.iter() {
                    if condition.evaluate(context)? {
                        return Ok(true)
                    }
                }
                Ok(false)
            }
            Condition::Xor { conditions} => {
                match conditions.len() {
                    1 => Ok(true),
                    _ => {
                        let first_result = conditions.head.evaluate(context)?;
                        for other in conditions.tail.iter() {
                            let other_result = other.evaluate(context)?;
                            if other_result != first_result {
                                return Ok(true)
                            }
                        }
                        Ok(false)
                    }
                }
            }
            Condition::Not { condition } => {
                Ok(!condition.evaluate(context)?)
            }
            Condition::True => Ok(true),
            Condition::False => Ok(false),
            Condition::Rule { rule } => {
                match rule.evaluate(context) {
                    Ok(RuleResult { action, .. }) => match action {
                        RuleAction::Accept => Ok(true),
                        RuleAction::Reject => Ok(false),
                        RuleAction::Continue => Ok(true),
                    }
                    Err(err) => Err(ConditionError::RuleError(Box::new(err))),
                }
            },
            Condition::RefAdd => match &context.change {
                Change::AddRef { .. } => Ok(true),
                Change::RemoveRef { .. } => Ok(false),
                Change::UpdateRef { .. } => Ok(false),
            },
            Condition::RefRemove => match &context.change {
                Change::AddRef { .. } => Ok(false),
                Change::RemoveRef { .. } => Ok(true),
                Change::UpdateRef { .. } => Ok(false),
            },
            Condition::RefUpdate => match &context.change {
                Change::AddRef { .. } => Ok(false),
                Change::RemoveRef { .. } => Ok(false),
                Change::UpdateRef { .. } => Ok(true),
            },
            Condition::LinearHistory => match &context.change {
                Change::AddRef { .. } => Ok(true),
                Change::RemoveRef { .. } => Ok(true),
                Change::UpdateRef { force, .. } => Ok(!force),
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RuleBranch {
    pub condition: Condition,
    pub rule: Rule,
}


#[derive(Debug)]
pub enum RuleError {
    ConditionError(ConditionError),
    WebhookError(HookError),
}

impl Display for RuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleError::ConditionError(err) => err.fmt(f),
            RuleError::WebhookError(err) => err.fmt(f),
        }
    }
}

#[derive(Debug)]
pub struct RuleResult {
    pub action: RuleAction,
    pub messages: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub enum RuleAction {
    Accept,
    Reject,
    Continue,
}

#[derive(Debug, Deserialize)]
pub struct OnRuleComplete {
    pub action: RuleAction,
    pub messages: Vec<String>,
}

trait OptionOnRuleComplete {
    fn to_rule_result(&self) -> RuleResult;
}

impl OptionOnRuleComplete for Option<OnRuleComplete> {
    fn to_rule_result(&self) -> RuleResult {
        match self {
            Some(OnRuleComplete { action, messages }) => {
                RuleResult { action: *action, messages: messages.clone() }
            }
            None => RuleResult { action: RuleAction::Continue, messages: vec![] },
        }
    }
}

#[derive(Debug, Deserialize)]
pub enum Rule {
    Chain {
        rules: NonEmpty<Box<Rule>>,
    },
    Select {
        first_of: Vec<RuleBranch>,
        default: Option<Box<Rule>>,
    },
    Webhook(WebhookRule),
    Accept {
        messages: Vec<String>,
    },
    Reject {
        messages: Vec<String>,
    },
    #[serde(untagged)]
    Conditional {
        condition: Condition,
        on_success: Option<OnRuleComplete>,
        on_failure: Option<OnRuleComplete>,
    },
}

impl Rule {
    pub fn evaluate(&self, context: &RuleContext) -> Result<RuleResult, RuleError> {
        match self {
            Rule::Chain { rules } => {
                let mut result: RuleResult = RuleResult { action: RuleAction::Reject, messages: vec![] };
                for rule in rules.iter() {
                    result = rule.evaluate(context)?;

                    match result.action {
                        RuleAction::Accept => break,
                        RuleAction::Reject => break,
                        RuleAction::Continue => continue,
                    }
                }

                if result.action == RuleAction::Continue {
                    result.action = RuleAction::Accept;
                }

                Ok(result)
            }
            Rule::Select { first_of, default } => {
                for RuleBranch { condition, rule } in first_of {
                    match condition.evaluate(context) {
                        Ok(true) => {
                            return rule.evaluate(context);
                        },
                        Ok(false) => continue,
                        Err(err) => return Err(RuleError::ConditionError(err)),
                    }
                }
                match default {
                    Some(rule) => {
                        rule.evaluate(context)
                    }
                    None => {
                        Ok(RuleResult { action: RuleAction::Reject, messages: vec![] })
                    }
                }
            }

            Rule::Conditional { condition, on_success, on_failure } => {
                match condition.evaluate(context) {
                    Ok(ok) => {
                        if ok {
                            Ok(on_success.to_rule_result())
                        } else {
                            Ok(on_failure.to_rule_result())
                        }
                    }
                    Err(err) => Err(RuleError::ConditionError(err)),
                }
            }
            Rule::Webhook(condition) => {
                let change = match context.change {
                    Change::AddRef { name, commit, git_data: GitData { patch, log, .. }, .. } => {
                        let patch = (*(*patch)).clone();
                        let log = (*(*log)).to_vec();
                        webbed_hook_core::webhook::Change::AddRef {
                            name: name.clone(),
                            commit: commit.clone(),
                            patch,
                            log: Some(log),
                        }
                    },
                    Change::RemoveRef { name, commit } => webbed_hook_core::webhook::Change::RemoveRef {
                        name: name.clone(),
                        commit: commit.clone(),
                    },
                    Change::UpdateRef { name, old_commit, new_commit, merge_base, force, git_data: GitData { patch, log, .. }, .. } => {
                        let patch = (*(*patch)).clone();
                        let log = (*(*log)).to_vec();
                        webbed_hook_core::webhook::Change::UpdateRef {
                            name: name.clone(),
                            old_commit: old_commit.clone(),
                            new_commit: new_commit.clone(),
                            merge_base: merge_base.clone(),
                            force: *force,
                            patch,
                            log: Some(log),
                        }
                    },
                };
                match perform_request(context.default_branch, context.push_options.into(), condition, vec![change]) {
                    Ok(WebhookResult(ok, WebhookResponse(messages))) => Ok(RuleResult {
                        action: if ok { RuleAction::Continue } else { RuleAction::Reject },
                        messages,
                    }),
                    Err(err) => Err(RuleError::WebhookError(err))
                }
            }
            Rule::Accept { messages } => Ok(RuleResult { action: RuleAction::Accept, messages: messages.clone() }),
            Rule::Reject { messages } => Ok(RuleResult { action: RuleAction::Reject, messages: messages.clone() }),
        }
    }
}
