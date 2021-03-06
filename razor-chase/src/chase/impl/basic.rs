//! Provides a "basic" implementation of the Chase.
//!
//! This implementation is used as a reference for the correctness of other implementations of the
//! [Chase].
//!
//! **Note**: The performance of `chase::impl::basic` is not a concern.
//!
//! [Chase]: ../../index.html#the-chase
//!
use crate::chase::*;
use razor_fol::syntax::*;
use std::{collections::{HashMap, HashSet}, fmt, iter};
use itertools::{Either, Itertools};

/// Is a straight forward implementation for [`WitnessTermTrait`], where elements are of type
/// [`E`].
///
/// [`WitnessTermTrait`]: ../../trait.WitnessTermTrait.html
/// [`E`]: ../../struct.E.html
#[derive(Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub enum WitnessTerm {
    /// Wraps an instance of [`E`], witnessing itself.
    ///
    /// [`E`]: ../../struct.E.html
    Elem { element: E },

    /// Wraps an instance of [`C`] as a witness term.
    ///
    /// [`C`]: ../../../formula/syntax/struct.C.html
    Const { constant: C },

    /// Corresponds to a composite witness term, made by applying an instance of [`F`] to a list of
    /// witness terms.
    ///
    /// [`F`]: ../../../formula/syntax/struct.F.html
    App { function: F, terms: Vec<Self> },
}

impl WitnessTerm {
    /// Given a `term` and an assignment function `assign` from variables of the term to elements
    /// of a [`Model`], constructs a [`WitnessTerm`].
    ///
    /// [`WitnessTerm`]: ./enum.WitnessTerm.html
    /// [`Model`]: ./struct.Model.html
    fn witness(term: &Term, assign: &impl Fn(&V) -> E) -> Self {
        match term {
            Term::Const { constant } => Self::Const { constant: constant.clone() },
            Term::Var { variable } => Self::Elem { element: assign(&variable) },
            Term::App { function, terms } => {
                let terms = terms
                    .iter()
                    .map(|t| Self::witness(t, assign))
                    .collect();
                Self::App { function: function.clone(), terms }
            }
        }
    }
}

impl WitnessTermTrait for WitnessTerm {
    type ElementType = E;
}

impl fmt::Display for WitnessTerm {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            Self::Elem { element } => write!(f, "{}", element),
            Self::Const { constant } => write!(f, "{}", constant),
            Self::App { function, terms } => {
                let ts: Vec<String> = terms.iter().map(|t| t.to_string()).collect();
                write!(f, "{}[{}]", function, ts.join(", "))
            }
        }
    }
}

impl fmt::Debug for WitnessTerm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

impl From<C> for WitnessTerm {
    fn from(constant: C) -> Self {
        WitnessTerm::Const { constant }
    }
}

impl From<E> for WitnessTerm {
    fn from(element: E) -> Self {
        WitnessTerm::Elem { element }
    }
}

impl FApp for WitnessTerm {
    fn apply(function: F, terms: Vec<Self>) -> Self {
        WitnessTerm::App { function, terms }
    }
}

/// Is a basic instance of [`ModelTrait`] with terms of type [`WitnessTerm`].
///
/// [`ModelTrait`]: ../../trait.ModelTrait.html
/// [`WitnessTerm`]: ./enum.WitnessTerm.html
pub struct Model {
    /// Is a unique identifier for this model.
    id: u64,

    /// Keeps track of the next index to assign to a new element of this model.
    element_index: i32,

    /// Maps *flat* witness terms to elements of this model.
    ///
    /// **Hint**: Flat (witness) terms are terms that do not contain any composite sub-terms,
    /// consisting of functions applications.
    rewrites: HashMap<WitnessTerm, E>,

    /// Contains a list of relational facts that are true in this model.
    facts: HashSet<Observation<WitnessTerm>>,

    /// Maintains a list of rewrite rules from elements to elements with which they have been
    /// identified.
    ///
    /// **Explanation**: When augmenting a model with a list of [observations] (such as observations
    /// that come from the head of a sequent being evaluated), identity observations are
    /// augmented by collapsing elements, that is, removing one element in favor of the other one.
    /// However, other augmenting observations may still point to an element that was removed as a
    /// result of augmenting an `Identity` observation.
    ///
    /// `equality_history` is used to keep track of identifications of elements temporarily during
    /// the time a model is being augmented in a [chase-step]. `equality_history` in a model becomes
    /// outdated after the [chase-step] ends.
    ///
    /// [observations]: ../../enum.Observation.html
    /// [chase-step]: ../../index.html#chase-step
    equality_history: HashMap<E, E>,
}

impl Model {
    /// Creates a new empty instance of this model.
    pub fn new() -> Self {
        Self {
            id: rand::random(),
            element_index: 0,
            rewrites: HashMap::new(),
            facts: HashSet::new(),
            equality_history: HashMap::new(),
        }
    }

    /// Applies the rewrite rules in `equality_history` of the receiver to reduce an element to
    /// the representative element of the equational class to which it belongs.
    fn history(&self, element: &E) -> E {
        let mut result = element;
        let mut next;
        while {
            next = self.equality_history.get(result);
            next.is_some() && next.unwrap() != result
        } { result = next.unwrap() }

        result.clone()
    }

    /// Creates a new element for the given `witness` and records that `witness` denotes the new
    /// element.
    fn new_element(&mut self, witness: WitnessTerm) -> E {
        let element = E(self.element_index);
        self.element_index = self.element_index + 1;
        self.rewrites.insert(witness, element.clone());
        element
    }

    /// Records the given `witness` in the receiver model and returns the element, denoted by
    /// `witness`.
    ///
    /// **Note**: `record` creates new elements that are denoted by `witness` and all sub-terms of
    /// `witness` and adds them to the domain of the receiver.
    fn record(&mut self, witness: &WitnessTerm) -> E {
        match witness {
            WitnessTerm::Elem { element } => {
                let element = self.history(element);
                if let Some(_) = self.domain().iter().find(|e| element.eq(e)) {
                    element.clone()
                } else {
                    panic!("something is wrong: element does not exist in the model's domain")
                }
            }
            WitnessTerm::Const { .. } => {
                if let Some(e) = self.rewrites.get(&witness) {
                    (*e).clone()
                } else {
                    self.new_element(witness.clone())
                }
            }
            WitnessTerm::App { function, terms } => {
                let terms: Vec<WitnessTerm> = terms
                    .into_iter()
                    .map(|t| self.record(t).into())
                    .collect();
                let witness = WitnessTerm::App { function: function.clone(), terms };
                if let Some(e) = self.rewrites.get(&witness) {
                    (*e).clone()
                } else {
                    self.new_element(witness)
                }
            }
        }
    }

    /// Replaces all instances of `from` with `to` in the `rewrite` of the receiver and is used
    /// when augmenting the model with an `Identity` [`Observation`].
    ///
    /// **Note**: When augmenting a model with an `Identity`, we simply replace all instances of one
    /// side of the identity (i.e., the element denoted by one [witness term]) with the other
    /// one.
    ///
    /// [`Observation`]: ../../enum.Observation.html
    /// [witness term]: ../../trait.WitnessTermTrait.html
    fn reduce_rewrites(&mut self, from: &E, to: &E) {
        let mut new_rewrite: HashMap<WitnessTerm, E> = HashMap::new();
        self.rewrites.iter().for_each(|(k, v)| {
            // k is a flat term and cannot be an element:
            let key = if let WitnessTerm::App { function, terms } = k {
                let mut new_terms: Vec<WitnessTerm> = Vec::new();
                terms.iter().for_each(|t| {
                    if let WitnessTerm::Elem { element } = t {
                        if element == to {
                            new_terms.push(WitnessTerm::Elem { element: from.clone() });
                        } else {
                            new_terms.push(t.clone());
                        }
                    } else {
                        new_terms.push(t.clone());
                    }
                });
                WitnessTerm::App { function: function.clone(), terms: new_terms }
            } else {
                k.clone()
            };

            let value = if v == to {
                from.clone()
            } else {
                v.clone()
            };
            new_rewrite.insert(key, value);
        });
        self.rewrites = new_rewrite;
    }

    /// Replaces all instances of `from` with `to` in the `facts` of the receiver and is used
    /// when augmenting the model with an `Identity` [`Observation`].
    ///
    /// **Note**: When augmenting a model with an identity, we simply replace all instances of one
    /// side of the identity (i.e., the element corresponding to its [witness term]) with the other
    /// one.
    ///
    /// [`Observation`]: ../../enum.Observation.html
    /// [witness term]: ../../trait.WitnessTermTrait.html
    fn reduce_facts(&mut self, from: &E, to: &E) {
        self.facts = self.facts.iter().map(|f| {
            if let Observation::Fact { ref relation, ref terms } = f {
                let terms: Vec<WitnessTerm> = terms.iter().map(|t| {
                    if let WitnessTerm::Elem { element } = t {
                        if element == to {
                            from.clone().into()
                        } else {
                            (*t).clone()
                        }
                    } else {
                        (*t).clone() // should never happen
                    }
                }).collect();
                Observation::Fact { relation: relation.clone(), terms }
            } else {
                f.clone() // should never happen
            }
        }).collect();
    }
}

impl Clone for Model {
    fn clone(&self) -> Self {
        Self {
            id: rand::random(),
            element_index: self.element_index.clone(),
            rewrites: self.rewrites.clone(),
            facts: self.facts.clone(),
            // In the `basic` implementation, a model is cloned after being processed in a
            // chase-step, so its `equality_history` does not need to persist after cloning it.
            equality_history: HashMap::new(),
        }
    }
}

impl ModelTrait for Model {
    type TermType = WitnessTerm;

    fn get_id(&self) -> u64 { self.id }

    fn domain(&self) -> Vec<&E> {
        self.rewrites.values().into_iter().unique().collect()
    }

    fn facts(&self) -> Vec<&Observation<Self::TermType>> {
        self.facts.iter().sorted().into_iter().dedup().collect()
    }

    fn observe(&mut self, observation: &Observation<WitnessTerm>) {
        match observation {
            Observation::Fact { relation, terms } => {
                let terms: Vec<WitnessTerm> = terms
                    .into_iter()
                    .map(|t| self.record(t).into())
                    .collect();
                let observation = Observation::Fact { relation: relation.clone(), terms };
                self.facts.insert(observation);
            }
            Observation::Identity { left, right } => {
                let left = self.record(left);
                let right = self.record(right);
                let (from, to) = if left > right {
                    (right, left)
                } else {
                    (left, right)
                };

                // Since the underlying ElementType of the WitnessTerm, used for constructing this
                // type of model is not a reference to an object (unlike chase::impl::reference),
                // the following two steps are necessary to guarantee correctness:
                self.reduce_rewrites(&from, &to);
                self.reduce_facts(&from, &to);

                self.equality_history.insert(to, from);
            }
        }
    }

    fn is_observed(&self, observation: &Observation<WitnessTerm>) -> bool {
        match observation {
            Observation::Fact { relation, terms } => {
                let terms: Vec<Option<&E>> = terms.iter().map(|t| self.element(t)).collect();
                if terms.iter().any(|e| e.is_none()) {
                    false
                } else {
                    let terms: Vec<WitnessTerm> = terms
                        .into_iter()
                        .map(|e| e.unwrap().clone().into())
                        .collect();
                    let obs = Observation::Fact { relation: relation.clone(), terms };
                    self.facts.iter().find(|f| obs.eq(f)).is_some()
                }
            }
            Observation::Identity { left, right } => {
                let left = self.element(left);
                left.is_some() && left == self.element(right)
            }
        }
    }

    fn witness(&self, element: &E) -> Vec<&WitnessTerm> {
        self.rewrites.iter()
            .filter(|(_, e)| *e == element)
            .map(|(t, _)| t)
            .collect()
    }

    fn element(&self, witness: &WitnessTerm) -> Option<&E> {
        match witness {
            WitnessTerm::Elem { element } => {
                self.domain().into_iter().find(|e| e.eq(&element))
            }
            WitnessTerm::Const { .. } => self.rewrites.get(witness).map(|e| e),
            WitnessTerm::App { function, terms } => {
                let terms: Vec<Option<&E>> = terms.iter().map(|t| self.element(t)).collect();
                if terms.iter().any(|e| e.is_none()) {
                    None
                } else {
                    let terms: Vec<WitnessTerm> = terms
                        .into_iter()
                        .map(|e| e.unwrap().clone().into())
                        .collect();
                    self.rewrites.get(&WitnessTerm::App { function: (*function).clone(), terms }).map(|e| e)
                }
            }
        }
    }
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let domain: Vec<String> = self.domain().into_iter().map(|e| e.to_string()).collect();
        let elements: Vec<String> = self.domain().iter().sorted().iter().map(|e| {
            let witnesses: Vec<String> = self.witness(e).iter().map(|w| w.to_string()).collect();
            let witnesses = witnesses.into_iter().sorted();
            format!("{} -> {}", witnesses.into_iter().sorted().join(", "), e)
        }).collect();
        let facts: Vec<String> = self.facts().into_iter().map(|e| e.to_string()).collect();
        write!(f, "Domain: {{{}}}\nElements:{}\nFacts: {}\n",
               domain.join(", "),
               elements.join(", "),
               facts.join(", "))
    }
}

/// Represents atomic formulae in [`Sequent`].
///
/// [`Sequent`]: ./struct.Sequent.html
#[derive(Clone)]
pub enum Literal {
    /// Represents an atomic literal, corresponding to an atomic [`Formula`] of variant [`Atom`].
    ///
    /// [`Formula`]: ../../../formula/syntax/enum.Formula.html
    /// [`Atom`]: ../../../formula/syntax/enum.Formula.html#variant.Atom
    Atm { predicate: Pred, terms: Vec<Term> },

    /// Represents a equality literal, corresponding to an atomic [`Formula`] of variant [`Equals`].
    ///
    /// [`Equals`]: ../../../formula/syntax/enum.Formula.html#variant.Equals
    Eql { left: Term, right: Term },
}

impl Literal {
    /// Builds the [body] of a [`Sequent`] from a [`Formula`].
    ///
    /// [`Sequent`]: ./struct.Sequent.html
    /// [body]: ./struct.Sequent.html#structfield.body_literals
    /// [`Formula`]: ../../../formula/syntax/enum.Formula.html
    fn build_body(formula: &Formula) -> Vec<Literal> {
        match formula {
            Formula::Top => vec![],
            Formula::Atom { predicate, terms } =>
                vec![Literal::Atm { predicate: predicate.clone(), terms: terms.to_vec() }],
            Formula::Equals { left, right } =>
                vec![Literal::Eql { left: left.clone(), right: right.clone() }],
            Formula::And { left, right } => {
                let mut left = Literal::build_body(left);
                let mut right = Literal::build_body(right);
                left.append(&mut right);
                left
            }
            _ => panic!("Something is wrong: expecting a geometric sequent in standard form.")
        }
    }

    /// Builds the [head] of a [`Sequent`] from a [`Formula`].
    ///
    /// [`Sequent`]: ./struct.Sequent.html
    /// [head]: ./struct.Sequent.html#structfield.head_literals
    /// [`Formula`]: ../../../formula/syntax/enum.Formula.html
    fn build_head(formula: &Formula) -> Vec<Vec<Literal>> {
        match formula {
            Formula::Top => vec![vec![]],
            Formula::Bottom => vec![],
            Formula::Atom { predicate, terms } =>
                vec![vec![Literal::Atm { predicate: predicate.clone(), terms: terms.to_vec() }]],
            Formula::Equals { left, right } =>
                vec![vec![Literal::Eql { left: left.clone(), right: right.clone() }]],
            Formula::And { left, right } => {
                let mut left = Literal::build_head(left);
                let mut right = Literal::build_head(right);
                if left.is_empty() {
                    left
                } else if right.is_empty() {
                    right
                } else if left.len() == 1 && right.len() == 1 {
                    let mut left = left.remove(0);
                    let mut right = right.remove(0);
                    left.append(&mut right);
                    vec![left]
                } else {
                    panic!("Something is wrong: expecting a geometric sequent in standard form.")
                }
            }
            Formula::Or { left, right } => {
                let mut left = Literal::build_head(left);
                let mut right = Literal::build_head(right);
                left.append(&mut right);
                left
            }
            _ => panic!("Something is wrong: expecting a geometric sequent in standard form.")
        }
    }
}

impl<'t> fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            Literal::Atm { predicate, terms } => {
                let ts: Vec<String> = terms.iter().map(|t| t.to_string()).collect();
                write!(f, "{}({})", predicate, ts.join(", "))
            }
            Literal::Eql { left, right } => write!(f, "{} = {}", left, right),
        }
    }
}

/// Is represented by a list of [`Literal`]s in the body and a list of list of `Literal`s in the
/// head.
///
/// [`Literal`]: ./enum.Literal.html
#[derive(Clone)]
pub struct Sequent {
    /// Is the list of free variables in the sequent and is used for memoization.
    pub free_vars: Vec<V>,

    /// Is the [`Formula`] from which the body of the sequent is built.
    ///
    /// [`Formula`]: ../../../formula/syntax/enum.Formula.html
    body: Formula,

    /// Is the [`Formula`] from which the head of the sequent is built.
    ///
    /// [`Formula`]: ../../../formula/syntax/enum.Formula.html
    head: Formula,

    /// Represents the body of the sequent as a list of [`Literal`]s. The literals in
    /// `body_literals` are assumed to be conjoined.
    ///
    /// [`Literal`]: ./enum.Literal.html
    ///
    /// **Note**: See [here](../../index.html#background) for more information about the structure
    /// of geometric sequents.
    pub body_literals: Vec<Literal>,

    /// Represents the head of the sequent as a list of list of [`Literal`]s. The literals in
    /// each sublist of `head_literals` are assumed to be conjoined where the sublists are
    /// disjointed with each other.
    ///
    /// [`Literal`]: ./enum.Literal.html
    ///
    /// **Note**: See [here](../../index.html#background) for more information about the structure
    /// of geometric sequents.
    pub head_literals: Vec<Vec<Literal>>,
}

impl From<&Formula> for Sequent {
    fn from(formula: &Formula) -> Self {
        match formula {
            Formula::Implies { left, right } => {
                let free_vars: Vec<V> = formula.free_vars().into_iter().map(|v| v.clone()).collect();
                let body_literals = Literal::build_body(left);
                let head_literals = Literal::build_head(right);
                let body = *left.clone();
                let head = *right.clone();
                Sequent { free_vars, body, head, body_literals, head_literals }
            }
            _ => panic!("Something is wrong: expecting a geometric sequent in standard form.")
        }
    }
}

impl fmt::Display for Sequent {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let body: Vec<String> = self.body_literals.iter().map(|l| l.to_string()).collect();
        let head: Vec<String> =
            self.head_literals.iter().map(|ls| {
                let ls: Vec<String> = ls.iter().map(|l| l.to_string()).collect();
                format!("[{}]", ls.join(", "))
            }).collect();
        write!(f, "[{}] -> [{}]", body.join(", "), head.join(", "))
    }
}

impl SequentTrait for Sequent {
    fn body(&self) -> Formula {
        self.body.clone()
    }

    fn head(&self) -> Formula {
        self.head.clone()
    }
}

/// Evaluates a [`Sequent`] in a [`Model`] within a [chase-step].
///
/// [`Sequent`]: ./struct.Sequent.html
/// [`Model`]: ./struct.Model.html
/// [chase-step]: ../../index.html#chase-step
pub struct Evaluator {}

impl<'s, Stg: StrategyTrait<Item=&'s Sequent>, B: BounderTrait> EvaluatorTrait<'s, Stg, B> for Evaluator {
    type Sequent = Sequent;
    type Model = Model;
    fn evaluate(&self, initial_model: &Model, strategy: &mut Stg, bounder: Option<&B>) -> Option<EvaluateResult<Model>> {
        let domain: Vec<&E> = initial_model.domain().clone();
        let domain_size = domain.len();
        for sequent in strategy {
            let vars = &sequent.free_vars;
            let vars_size = vars.len();
            if domain_size == 0 && vars_size > 0 {
                continue; // empty models can only be extended with sequents with no free variables.
            }

            // maintain a list of indices into the model elements to which variables are mapped
            let mut assignment: Vec<usize> = iter::repeat(0).take(vars_size).collect();

            // try all the variable assignments to the elements of the model
            // (notice the do-while pattern)
            while {
                // construct a map from variables to elements
                let mut assignment_map: HashMap<&V, E> = HashMap::new();
                for i in 0..vars_size {
                    assignment_map.insert(vars.get(i).unwrap(), (*domain.get(assignment[i]).unwrap()).clone());
                }
                // construct a "characteristic function" for the assignment map
                let assignment_func = |v: &V| assignment_map.get(v).unwrap().clone();

                // lift the variable assignments to literals (used to create observations)
                let observe_literal = make_observe_literal(assignment_func);

                // build body and head observations
                let body: Vec<Observation<WitnessTerm>> = sequent.body_literals
                    .iter()
                    .map(&observe_literal)
                    .collect();
                let head: Vec<Vec<Observation<WitnessTerm>>> = sequent.head_literals
                    .iter()
                    .map(|l| l.iter().map(&observe_literal).collect())
                    .collect();

                // if all body observations are true in the model but not all the head observations
                // are true, extend the model:
                if body.iter().all(|o| initial_model.is_observed(o))
                    && !head.iter().any(|os| os.iter().all(|o| initial_model.is_observed(o))) {
                    if head.is_empty() {
                        return None; // the chase fails if the head is empty (false)
                    } else {
                        // if there is a bounder, only extend models that are not out of the given bound:
                        let models: Vec<Either<Model, Model>> = if let Some(bounder) = bounder {
                            let extend = make_bounded_extend(bounder, initial_model);
                            head.iter().map(extend).collect()
                        } else {
                            let extend = make_extend(initial_model);
                            head.iter().map(extend).collect()
                        };

                        let result = EvaluateResult::from(models);
                        if !result.empty() {
                            // this evaluator instantiates the first matching sequent with the first
                            // matching assignment (unlike impl::batch.rs)
                            return Some(result);
                        }
                    }
                }

                // try the next variable assignment
                domain_size > 0 && next_assignment(&mut assignment, domain_size - 1)
            } {}
        }
        Some(EvaluateResult::new()) // if none of the assignments apply, the model is complete already
    }
}

// Returns a closure that returns a cloned extension of the given model, extended by a given set of
// observations.
fn make_extend<'m>(
    model: &'m Model
) -> impl FnMut(&'m Vec<Observation<WitnessTerm>>) -> Either<Model, Model>
{
    move |os: &'m Vec<Observation<WitnessTerm>>| {
        let mut model = model.clone();
        os.iter().foreach(|o| model.observe(o));
        Either::Left(model)
    }
}

// Returns a closure that returns a cloned extension of the given model, extended by a given set of
// observations. Unlike `make_extend`, `make_bounded_extend` extends the model with respect to a
// bounder: a model wrapped in `Either::Right` has not reached the bounds while a model wrapped in
// `Either::Left` has reached the bounds provided by `bounder`.
fn make_bounded_extend<'m, B: BounderTrait>(
    bounder: &'m B,
    model: &'m Model,
) -> impl FnMut(&'m Vec<Observation<WitnessTerm>>) -> Either<Model, Model>
{
    move |os: &Vec<Observation<WitnessTerm>>| {
        let mut model = model.clone();
        let mut modified = false;
        os.iter().foreach(|o| {
            if bounder.bound(&model, o) {
                if !model.is_observed(o) {
                    modified = true;
                }
            } else {
                if !model.is_observed(o) {
                    model.observe(o);
                }
            }
        });
        if modified {
            Either::Right(model)
        } else {
            Either::Left(model)
        }
    }
}

// Given an function from variables to elements of a model, returns a closure that lift the variable
// assignments to literals of a sequent, returning observations.
fn make_observe_literal(assignment_func: impl Fn(&V) -> E)
                        -> impl Fn(&Literal) -> Observation<WitnessTerm> {
    move |lit: &Literal| {
        match lit {
            Literal::Atm { predicate, terms } => {
                let terms = terms
                    .into_iter()
                    .map(|t| WitnessTerm::witness(t, &assignment_func))
                    .collect();
                Observation::Fact { relation: Rel(predicate.0.clone()), terms }
            }
            Literal::Eql { left, right } => {
                let left = WitnessTerm::witness(left, &assignment_func);
                let right = WitnessTerm::witness(right, &assignment_func);
                Observation::Identity { left, right }
            }
        }
    }
}

// Implements a counter to enumerate all the possible assignments of a domain of elements to free
// variables of a sequent. It mutates the given a list of indices, corresponding to mapping of each
// position to an element of a domain to the next assignment. Returns true if a next assignment
// exists and false otherwise.
fn next_assignment(vec: &mut Vec<usize>, last: usize) -> bool {
    let len = vec.len();
    for i in 0..len {
        if vec[i] != last {
            vec[i] += 1;
            return true;
        } else {
            vec[i] = 0;
        }
    }
    false
}

#[cfg(test)]
mod test_basic {
    use super::*;
    use crate::test_prelude::*;
    use std::iter::FromIterator;

    // Witness Elements
    pub fn _e_0() -> WitnessTerm { e_0().into() }

    pub fn _e_1() -> WitnessTerm { e_1().into() }

    pub fn _e_2() -> WitnessTerm { e_2().into() }

    pub fn _e_3() -> WitnessTerm { e_3().into() }

    pub fn _e_4() -> WitnessTerm { e_4().into() }

    // Witness Constants
    pub fn _a_() -> WitnessTerm { WitnessTerm::Const { constant: _a() } }

    pub fn _b_() -> WitnessTerm { WitnessTerm::Const { constant: _b() } }

    pub fn _c_() -> WitnessTerm { WitnessTerm::Const { constant: _c() } }

    pub fn _d_() -> WitnessTerm { WitnessTerm::Const { constant: _d() } }

    #[test]
    fn test_witness_const() {
        assert_eq!(_a_(), _a().into());
        assert_eq!("'a", _a_().to_string());
    }

    #[test]
    fn test_observation() {
        assert_eq!("<R(e#0)>", _R_().app1(_e_0()).to_string());
        assert_eq!("<R(e#0, e#1, e#2)>", _R_().app3(_e_0(), _e_1(), _e_2()).to_string());
        assert_eq!("<e#0 = e#1>", _e_0().equals(_e_1()).to_string());
    }

    #[test]
    fn test_empty_model() {
        let model = Model::new();
        let empty_domain: Vec<&E> = Vec::new();
        let empty_facts: Vec<&Observation<WitnessTerm>> = Vec::new();
        assert_eq!(empty_domain, model.domain());
        assert_eq_sets(&empty_facts, &model.facts());
    }

    #[test]
    fn test_witness_app() {
        let f_0: WitnessTerm = f().app0();
        assert_eq!("f[]", f_0.to_string());
        assert_eq!("f['c]", f().app1(_c_()).to_string());
        let g_0: WitnessTerm = g().app0();
        assert_eq!("f[g[]]", f().app1(g_0).to_string());
        assert_eq!("f['c, g['d]]", f().app2(_c_(), g().app1(_d_())).to_string());
    }

    #[test]
    fn test_observe() {
        {
            let mut model = Model::new();
            model.observe(&_R_().app0());
            assert_eq_sets(&Vec::from_iter(vec![_R_().app0()].iter()), &model.facts());
            assert!(model.is_observed(&_R_().app0()));
        }
        {
            let mut model = Model::new();
            model.observe(&_R_().app1(_c_()));
            assert_eq_sets(&Vec::from_iter(vec![&e_0()]), &model.domain());
            assert_eq_sets(&Vec::from_iter(vec![_R_().app1(_e_0())].iter()), &model.facts());
            assert!(model.is_observed(&_R_().app1(_c_())));
            assert!(model.is_observed(&_R_().app1(_e_0())));
            assert!(!model.is_observed(&_R_().app1(_e_1())));
            assert_eq_sets(&Vec::from_iter(vec![&_c_()]), &model.witness(&e_0()));
        }
        {
            let mut model = Model::new();
            model.observe(&_a_().equals(_b_()));
            assert_eq_sets(&Vec::from_iter(vec![&e_0()]), &model.domain());
            let empty_facts: Vec<&Observation<WitnessTerm>> = Vec::new();
            assert_eq_sets(&empty_facts, &model.facts());
            assert_eq_sets(&Vec::from_iter(vec![&_a_(), &_b_()]), &model.witness(&e_0()));
        }
        {
            let mut model = Model::new();
            model.observe(&_a_().equals(_a_()));
            assert_eq_sets(&Vec::from_iter(vec![&e_0()]), &model.domain());
            let empty_facts: Vec<&Observation<WitnessTerm>> = Vec::new();
            assert_eq_sets(&empty_facts, &model.facts());
            assert_eq_sets(&Vec::from_iter(vec![&_a_()]), &model.witness(&e_0()));
        }
        {
            let mut model = Model::new();
            model.observe(&_P_().app1(_a_()));
            model.observe(&_Q_().app1(_b_()));
            model.observe(&_a_().equals(_b_()));
            assert_eq_sets(&Vec::from_iter(vec![&e_0()]), &model.domain());
            assert_eq_sets(&Vec::from_iter(vec![_P_().app1(_e_0()), _Q_().app1(_e_0())].iter()), &model.facts());
            assert!(model.is_observed(&_P_().app1(_e_0())));
            assert!(model.is_observed(&_P_().app1(_a_())));
            assert!(model.is_observed(&_P_().app1(_b_())));
            assert!(model.is_observed(&_Q_().app1(_e_0())));
            assert!(model.is_observed(&_Q_().app1(_a_())));
            assert!(model.is_observed(&_Q_().app1(_b_())));
            assert_eq_sets(&Vec::from_iter(vec![&_a_(), &_b_()]), &model.witness(&e_0()));
        }
        {
            let mut model = Model::new();
            model.observe(&_R_().app1(f().app1(_c_())));
            assert_eq_sets(&Vec::from_iter(vec![&e_0(), &e_1()]), &model.domain());
            assert_eq_sets(&Vec::from_iter(vec![_R_().app1(_e_1())].iter()), &model.facts());
            assert!(model.is_observed(&_R_().app1(_e_1())));
            assert!(model.is_observed(&_R_().app1(f().app1(_c_()))));
            assert_eq_sets(&Vec::from_iter(vec![&_c_()]), &model.witness(&e_0()));
            assert_eq_sets(&Vec::from_iter(vec![&f().app1(_e_0())]), &model.witness(&e_1()));
        }
        {
            let mut model = Model::new();
            model.observe(&_R_().app2(_a_(), _b_()));
            assert_eq_sets(&Vec::from_iter(vec![&e_0(), &e_1()]), &model.domain());
            assert_eq_sets(&Vec::from_iter(vec![_R_().app2(_e_0(), _e_1())].iter()), &model.facts());
            assert!(model.is_observed(&_R_().app2(_e_0(), _e_1())));
            assert!(!model.is_observed(&_R_().app2(_e_0(), _e_0())));
            assert_eq_sets(&Vec::from_iter(vec![&_a_()]), &model.witness(&e_0()));
            assert_eq_sets(&Vec::from_iter(vec![&_b_()]), &model.witness(&e_1()));
        }
        {
            let mut model = Model::new();
            model.observe(&_R_().app2(f().app1(_c_()), g().app1(f().app1(_c_()))));
            assert_eq_sets(&Vec::from_iter(vec![&e_0(), &e_1(), &e_2()]), &model.domain());
            assert_eq_sets(&Vec::from_iter(vec![_R_().app2(_e_1(), _e_2())].iter()), &model.facts());
            assert!(model.is_observed(&_R_().app2(_e_1(), _e_2())));
            assert!(model.is_observed(&_R_().app2(f().app1(_c_()), g().app1(f().app1(_c_())))));
            assert!(model.is_observed(&_R_().app2(f().app1(_c_()), _e_2())));
            assert_eq_sets(&Vec::from_iter(vec![&_c_()]), &model.witness(&e_0()));
            assert_eq_sets(&Vec::from_iter(vec![&f().app1(_e_0())]), &model.witness(&e_1()));
            assert_eq_sets(&Vec::from_iter(vec![&g().app1(_e_1())]), &model.witness(&e_2()));
        }
        {
            let mut model = Model::new();
            model.observe(&_R_().app2(_a_(), _b_()));
            model.observe(&_S_().app2(_c_(), _d_()));
            assert_eq_sets(&Vec::from_iter(vec![&e_0(), &e_1(), &e_2(), &e_3()]), &model.domain());
            assert_eq_sets(&Vec::from_iter(vec![_R_().app2(_e_0(), _e_1())
                                                , _S_().app2(_e_2(), _e_3())
            ].iter()), &model.facts());
        }
        {
            let mut model = Model::new();
            model.observe(&_R_().app2(_a_(), f().app1(_a_())));
            model.observe(&_S_().app1(_b_()));
            model.observe(&_R_().app2(g().app1(f().app1(_a_())), _b_()));
            model.observe(&_S_().app1(_c_()));
            assert_eq_sets(&Vec::from_iter(vec![&e_0(), &e_1(), &e_2(), &e_3(), &e_4()]), &model.domain());
            assert_eq_sets(&Vec::from_iter(vec![_R_().app2(_e_0(), _e_1())
                                                , _S_().app1(_e_4())
                                                , _S_().app1(_e_2())
                                                , _R_().app2(_e_3(), _e_2())
            ].iter()), &model.facts());
        }
    }

    #[test]
    #[should_panic]
    fn test_observe_missing_element() {
        let mut model = Model::new();
        model.observe(&_R_().app1(_e_0()));
    }

    #[test]
    fn test_build_sequent() {
        assert_debug_string("[] -> [[]]",
                            Sequent::from(&"true -> true".parse().unwrap()));
        assert_debug_string("[] -> [[]]",
                            Sequent::from(&"true -> true & true".parse().unwrap()));
        assert_debug_string("[] -> [[], []]",
                            Sequent::from(&"true -> true | true".parse().unwrap()));
        assert_debug_string("[] -> []",
                            Sequent::from(&"true -> false".parse().unwrap()));
        assert_debug_string("[] -> []",
                            Sequent::from(&"true -> false & true".parse().unwrap()));
        assert_debug_string("[] -> []",
                            Sequent::from(&"true -> true & false".parse().unwrap()));
        assert_debug_string("[] -> [[]]",
                            Sequent::from(&"true -> true | false".parse().unwrap()));
        assert_debug_string("[] -> [[]]",
                            Sequent::from(&"true -> false | true".parse().unwrap()));
        assert_debug_string("[P(x)] -> [[Q(x)]]",
                            Sequent::from(&"P(x) -> Q(x)".parse().unwrap()));
        assert_debug_string("[P(x), Q(x)] -> [[Q(y)]]",
                            Sequent::from(&"P(x) & Q(x) -> Q(y)".parse().unwrap()));
        assert_debug_string("[P(x), Q(x)] -> [[Q(x)], [R(z), S(z)]]",
                            Sequent::from(&"P(x) & Q(x) -> Q(x) | (R(z) & S(z))".parse().unwrap()));
        assert_debug_string("[] -> [[P(x), Q(x)], [P(y), Q(y)], [P(z), Q(z)]]",
                            Sequent::from(&"true -> (P(x) & Q(x)) | (P(y) & Q(y)) | (P(z) & Q(z))".parse().unwrap()));
    }

    #[test]
    #[should_panic]
    fn test_build_invalid_sequent_1() {
        Sequent::from(&"true".parse().unwrap());
    }

    #[test]
    #[should_panic]
    fn test_build_invalid_sequent_2() {
        Sequent::from(&"false".parse().unwrap());
    }

    #[test]
    #[should_panic]
    fn test_build_invalid_sequent_3() {
        Sequent::from(&"false -> true".parse().unwrap());
    }

    #[test]
    #[should_panic]
    fn test_build_invalid_sequent_4() {
        Sequent::from(&"(P(x) | Q(x)) -> R(x)".parse().unwrap());
    }

    #[test]
    #[should_panic]
    fn test_build_invalid_sequent_5() {
        Sequent::from(&"P(x) -> R(x) & (Q(z) | R(z))".parse().unwrap());
    }

    #[test]
    #[should_panic]
    fn test_build_invalid_sequent_6() {
        Sequent::from(&"P(x) -> ?x. Q(x)".parse().unwrap());
    }

    #[test]
    #[should_panic]
    fn test_build_invalid_sequent_7() {
        Sequent::from(&"?x.Q(x) -> P(x)".parse().unwrap());
    }

    #[test]
    #[should_panic]
    fn test_build_invalid_sequent_8() {
        Sequent::from(&"true -> ~false".parse().unwrap());
    }

    #[test]
    #[should_panic]
    fn test_build_invalid_sequent_9() {
        Sequent::from(&"true -> ~true".parse().unwrap());
    }

    #[test]
    #[should_panic]
    fn test_build_invalid_sequent_10() {
        Sequent::from(&"~P(x) -> ~Q(x)".parse().unwrap());
    }

    #[test]
    fn test_core() {
        assert_eq!("Domain: {e#0}\n\
                      Facts: <P(e#0)>\n\
                      'a -> e#0",
                   print_basic_models(solve_basic(&&read_theory_from_file("../theories/core/thy0.raz"))));
        assert_eq!("Domain: {e#0, e#1}\n\
                       Facts: <P(e#0)>, <P(e#1)>\n\
                       'a -> e#0\n\
                       'b -> e#1",
                   print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy1.raz"))));
        assert_eq!("Domain: {e#0}\n\
                       Facts: <P(e#0)>, <Q(e#0)>\n\
                       'a -> e#0",
                   print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy2.raz"))));
        assert_eq!("Domain: {e#0, e#1}\n\
                       Facts: <R(e#0, e#1)>\n\
                       'sk#0 -> e#0\n\
                       'sk#1 -> e#1",
                   print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy3.raz"))));
        assert_eq!("Domain: {e#0}\n\
                Facts: \n\
                'a, 'b -> e#0",
                   print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy4.raz"))));
        assert_eq!("Domain: {e#0, e#1}\n\
                       Facts: <P(e#0, e#1)>\n\
                       'a -> e#0\n\
                       'b -> e#1",
                   print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy5.raz"))));
        assert_eq!("Domain: {e#0, e#1}\n\
                       Facts: <P(e#1)>\n\
                       'a -> e#0\n\
                       f[e#0] -> e#1",
                   print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy6.raz"))));
        assert_eq!("Domain: {e#0}\n\
                       Facts: <P(e#0)>, <Q(e#0)>, <R(e#0)>\n\
                       'a -> e#0",
                   print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy7.raz"))));
        assert_eq!("Domain: {e#0}\n\
                       Facts: <P(e#0)>\n\
                       'a -> e#0\n\
                       -- -- -- -- -- -- -- -- -- --\n\
                       Domain: {e#0}\n\
                       Facts: <Q(e#0)>\n\
                       'b -> e#0\n\
                       -- -- -- -- -- -- -- -- -- --\n\
                       Domain: {e#0}\n\
                       Facts: <R(e#0)>\n\
                       'c -> e#0",
                   print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy8.raz"))));
        assert_eq!("Domain: {e#0}\n\
                       Facts: <P(e#0)>, <Q(e#0)>\n\
                       'a, 'b -> e#0",
                   print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy9.raz"))));
        assert_eq!("Domain: {e#0}\n\
                       Facts: <P(e#0)>, <R(e#0)>\n\
                       'a -> e#0\n\
                       -- -- -- -- -- -- -- -- -- --\n\
                       Domain: {e#0}\n\
                       Facts: <Q(e#0)>, <S(e#0)>\n\
                       'b -> e#0",
                   print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy10.raz"))));
        assert_eq!("Domain: {}\n\
                       Facts: \n",
                   print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy11.raz"))));
        assert_eq!("Domain: {}\n\
                       Facts: \n",
                   print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy12.raz"))));
        assert_eq!("", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy13.raz"))));
        assert_eq!("Domain: {e#0}\n\
                       Facts: <Q(e#0)>\n\
                       'b -> e#0", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy14.raz"))));
        assert_eq!("", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy15.raz"))));
        assert_eq!("Domain: {e#0}\n\
                       Facts: <P(e#0, e#0)>, <Q(e#0)>\n\
                       'c -> e#0", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy16.raz"))));
        assert_eq!("Domain: {e#0, e#1, e#2}\n\
                       Facts: <P(e#0, e#0)>, <P(e#1, e#2)>, <Q(e#0)>\n\
                       'c -> e#0\n\
                       'a -> e#1\n\
                       'b -> e#2", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy17.raz"))));
        assert_eq!("Domain: {e#0, e#1, e#2}\n\
                       Facts: <P(e#0, e#1)>, <P(e#2, e#2)>, <Q(e#2)>\n\
                       'a -> e#0\n\
                       'b -> e#1\n\
                       'c -> e#2", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy18.raz"))));
        assert_eq!("Domain: {e#0, e#1, e#10, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
                       Facts: \n\
                       'a -> e#0\n\
                       f[e#0] -> e#1\n\
                       f[e#1] -> e#2\n\
                       f[e#2] -> e#3\n\
                       f[e#3] -> e#4\n\
                       f[e#4] -> e#5\n\
                       f[e#5] -> e#6\n\
                       f[e#6] -> e#7\n\
                       f[e#7] -> e#8\n\
                       f[e#8] -> e#9\n\
                       'b, f[e#9] -> e#10", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy19.raz"))));
        assert_eq!("Domain: {e#0, e#1, e#10, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
                       Facts: <P(e#0)>, <P(e#1)>, <P(e#2)>, <P(e#3)>, <P(e#4)>, <P(e#5)>, <P(e#6)>, <P(e#7)>, <P(e#8)>, <P(e#9)>\n\
                       'a -> e#0\n\
                       f[e#0] -> e#1\n\
                       f[e#1] -> e#2\n\
                       f[e#2] -> e#3\n\
                       f[e#3] -> e#4\n\
                       f[e#4] -> e#5\n\
                       f[e#5] -> e#6\n\
                       f[e#6] -> e#7\n\
                       f[e#7] -> e#8\n\
                       f[e#8] -> e#9\n\
                       'b, f[e#9] -> e#10", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy20.raz"))));
        assert_eq!("Domain: {e#0, e#1, e#10, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
                       Facts: <P(e#0)>, <P(e#1)>, <P(e#2)>, <P(e#3)>, <P(e#4)>, <P(e#5)>, <P(e#6)>, <P(e#7)>, <P(e#8)>\n\
                       'a -> e#0\n\
                       f[e#0] -> e#1\n\
                       f[e#1] -> e#2\n\
                       f[e#2] -> e#3\n\
                       f[e#3] -> e#4\n\
                       f[e#4] -> e#5\n\
                       f[e#5] -> e#6\n\
                       f[e#6] -> e#7\n\
                       f[e#7] -> e#8\n\
                       f[e#8] -> e#9\n\
                       'b, f[e#9] -> e#10", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy21.raz"))));
        assert_eq!("Domain: {e#0}\n\
                Facts: <P(e#0)>, <Q(e#0)>, <R(e#0)>\n\
                'a -> e#0", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy22.raz"))));
        assert_eq!("Domain: {e#0}\n\
                       Facts: <P(e#0)>, <Q(e#0)>, <R(e#0)>, <S(e#0)>\n\
                       'sk#0, 'sk#1, 'sk#2 -> e#0", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy23.raz"))));
        assert_eq!("Domain: {e#0}\n\
                       Facts: <P(e#0)>, <Q(e#0)>, <R(e#0)>, <S(e#0)>, <T(e#0)>\n\
                       'sk#0, 'sk#1, 'sk#2, 'sk#3 -> e#0", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy24.raz"))));
        assert_eq!("Domain: {e#0, e#1, e#2, e#3}\n\
                       Facts: <P(e#0)>, <Q(e#1)>, <R(e#2)>, <S(e#3)>\n\
                       'sk#0 -> e#0\n\
                       'sk#1 -> e#1\n\
                       'sk#2 -> e#2\n\
                       'sk#3 -> e#3", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy25.raz"))));
        assert_eq!("Domain: {e#0}\n\
                       Facts: <P(e#0)>\n\
                       'sk#0 -> e#0\n\
                       -- -- -- -- -- -- -- -- -- --\n\
                       Domain: {e#0}\n\
                       Facts: <P(e#0)>\n\
                       'sk#1 -> e#0", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy26.raz"))));
        assert_eq!("", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy27.raz"))));
        assert_eq!("Domain: {}\n\
        Facts: <T()>, <V()>\n\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {}\n\
        Facts: <U()>, <V()>\n", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy28.raz"))));
        assert_eq!("Domain: {}\n\
        Facts: <P()>\n\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {}\n\
        Facts: <Q()>, <R()>, <T()>, <V()>\n\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {}\n\
        Facts: <Q()>, <R()>, <U()>, <V()>\n\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {}\n\
        Facts: <Q()>, <S()>, <W()>\n\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {}\n\
        Facts: <Q()>, <S()>, <X()>\n\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {}\n\
        Facts: <Q()>, <S()>, <Y()>\n", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy29.raz"))));
        assert_eq!("", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy30.raz"))));
        assert_eq!("Domain: {e#0}\n\
                       Facts: <Q(e#0, e#0)>, <R(e#0)>, <U(e#0)>\n\
                       'sk#0 -> e#0", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy31.raz"))));
        assert_eq!("Domain: {e#0, e#1}\n\
        Facts: <Q(e#0, e#1)>, <R(e#0)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy32.raz"))));
        assert_eq!("Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P1(e#1)>, <P11(e#2)>, <P111(e#3)>, <P1111(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#3[e#1] -> e#2\n\
        sk#7[e#2] -> e#3\n\
        sk#15[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P1(e#1)>, <P11(e#2)>, <P111(e#3)>, <P1112(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#3[e#1] -> e#2\n\
        sk#7[e#2] -> e#3\n\
        sk#15[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P1(e#1)>, <P11(e#2)>, <P112(e#3)>, <P1121(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#3[e#1] -> e#2\n\
        sk#7[e#2] -> e#3\n\
        sk#17[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P1(e#1)>, <P11(e#2)>, <P112(e#3)>, <P1122(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#3[e#1] -> e#2\n\
        sk#7[e#2] -> e#3\n\
        sk#17[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P1(e#1)>, <P12(e#2)>, <P121(e#3)>, <P1211(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#3[e#1] -> e#2\n\
        sk#9[e#2] -> e#3\n\
        sk#19[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P1(e#1)>, <P12(e#2)>, <P121(e#3)>, <P1212(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#3[e#1] -> e#2\n\
        sk#9[e#2] -> e#3\n\
        sk#19[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P1(e#1)>, <P12(e#2)>, <P122(e#3)>, <P1221(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#3[e#1] -> e#2\n\
        sk#9[e#2] -> e#3\n\
        sk#21[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P1(e#1)>, <P12(e#2)>, <P122(e#3)>, <P1222(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#3[e#1] -> e#2\n\
        sk#9[e#2] -> e#3\n\
        sk#21[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P2(e#1)>, <P21(e#2)>, <P211(e#3)>, <P2111(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#5[e#1] -> e#2\n\
        sk#11[e#2] -> e#3\n\
        sk#23[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P2(e#1)>, <P21(e#2)>, <P211(e#3)>, <P2112(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#5[e#1] -> e#2\n\
        sk#11[e#2] -> e#3\n\
        sk#23[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P2(e#1)>, <P21(e#2)>, <P212(e#3)>, <P2121(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#5[e#1] -> e#2\n\
        sk#11[e#2] -> e#3\n\
        sk#25[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P2(e#1)>, <P21(e#2)>, <P212(e#3)>, <P2122(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#5[e#1] -> e#2\n\
        sk#11[e#2] -> e#3\n\
        sk#25[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P2(e#1)>, <P22(e#2)>, <P221(e#3)>, <P2211(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#5[e#1] -> e#2\n\
        sk#13[e#2] -> e#3\n\
        sk#27[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P2(e#1)>, <P22(e#2)>, <P221(e#3)>, <P2212(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#5[e#1] -> e#2\n\
        sk#13[e#2] -> e#3\n\
        sk#27[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P2(e#1)>, <P22(e#2)>, <P222(e#3)>, <P2221(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#5[e#1] -> e#2\n\
        sk#13[e#2] -> e#3\n\
        sk#29[e#3] -> e#4\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#0)>, <P2(e#1)>, <P22(e#2)>, <P222(e#3)>, <P2222(e#4)>\n\
        'sk#0 -> e#0\n\
        sk#1[e#0] -> e#1\n\
        sk#5[e#1] -> e#2\n\
        sk#13[e#2] -> e#3\n\
        sk#29[e#3] -> e#4", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy35.raz"))));
        assert_eq!("Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q1(e#2, e#3)>, <Q11(e#4, e#5)>, <Q111(e#6, e#7)>, <Q1111(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#6[e#2, e#3] -> e#4\n\
        sk#7[e#2, e#3] -> e#5\n\
        sk#14[e#4, e#5] -> e#6\n\
        sk#15[e#4, e#5] -> e#7\n\
        sk#30[e#6, e#7] -> e#8\n\
        sk#31[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q1(e#2, e#3)>, <Q11(e#4, e#5)>, <Q111(e#6, e#7)>, <Q1112(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#6[e#2, e#3] -> e#4\n\
        sk#7[e#2, e#3] -> e#5\n\
        sk#14[e#4, e#5] -> e#6\n\
        sk#15[e#4, e#5] -> e#7\n\
        sk#30[e#6, e#7] -> e#8\n\
        sk#31[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q1(e#2, e#3)>, <Q11(e#4, e#5)>, <Q112(e#6, e#7)>, <Q1121(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#6[e#2, e#3] -> e#4\n\
        sk#7[e#2, e#3] -> e#5\n\
        sk#14[e#4, e#5] -> e#6\n\
        sk#15[e#4, e#5] -> e#7\n\
        sk#34[e#6, e#7] -> e#8\n\
        sk#35[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q1(e#2, e#3)>, <Q11(e#4, e#5)>, <Q112(e#6, e#7)>, <Q1122(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#6[e#2, e#3] -> e#4\n\
        sk#7[e#2, e#3] -> e#5\n\
        sk#14[e#4, e#5] -> e#6\n\
        sk#15[e#4, e#5] -> e#7\n\
        sk#34[e#6, e#7] -> e#8\n\
        sk#35[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q1(e#2, e#3)>, <Q12(e#4, e#5)>, <Q121(e#6, e#7)>, <Q1211(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#6[e#2, e#3] -> e#4\n\
        sk#7[e#2, e#3] -> e#5\n\
        sk#18[e#4, e#5] -> e#6\n\
        sk#19[e#4, e#5] -> e#7\n\
        sk#38[e#6, e#7] -> e#8\n\
        sk#39[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q1(e#2, e#3)>, <Q12(e#4, e#5)>, <Q121(e#6, e#7)>, <Q1212(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#6[e#2, e#3] -> e#4\n\
        sk#7[e#2, e#3] -> e#5\n\
        sk#18[e#4, e#5] -> e#6\n\
        sk#19[e#4, e#5] -> e#7\n\
        sk#38[e#6, e#7] -> e#8\n\
        sk#39[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q1(e#2, e#3)>, <Q12(e#4, e#5)>, <Q122(e#6, e#7)>, <Q1221(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#6[e#2, e#3] -> e#4\n\
        sk#7[e#2, e#3] -> e#5\n\
        sk#18[e#4, e#5] -> e#6\n\
        sk#19[e#4, e#5] -> e#7\n\
        sk#42[e#6, e#7] -> e#8\n\
        sk#43[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q1(e#2, e#3)>, <Q12(e#4, e#5)>, <Q122(e#6, e#7)>, <Q1222(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#6[e#2, e#3] -> e#4\n\
        sk#7[e#2, e#3] -> e#5\n\
        sk#18[e#4, e#5] -> e#6\n\
        sk#19[e#4, e#5] -> e#7\n\
        sk#42[e#6, e#7] -> e#8\n\
        sk#43[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q2(e#2, e#3)>, <Q21(e#4, e#5)>, <Q211(e#6, e#7)>, <Q2111(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#10[e#2, e#3] -> e#4\n\
        sk#11[e#2, e#3] -> e#5\n\
        sk#22[e#4, e#5] -> e#6\n\
        sk#23[e#4, e#5] -> e#7\n\
        sk#46[e#6, e#7] -> e#8\n\
        sk#47[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q2(e#2, e#3)>, <Q21(e#4, e#5)>, <Q211(e#6, e#7)>, <Q2112(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#10[e#2, e#3] -> e#4\n\
        sk#11[e#2, e#3] -> e#5\n\
        sk#22[e#4, e#5] -> e#6\n\
        sk#23[e#4, e#5] -> e#7\n\
        sk#46[e#6, e#7] -> e#8\n\
        sk#47[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q2(e#2, e#3)>, <Q21(e#4, e#5)>, <Q212(e#6, e#7)>, <Q2121(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#10[e#2, e#3] -> e#4\n\
        sk#11[e#2, e#3] -> e#5\n\
        sk#22[e#4, e#5] -> e#6\n\
        sk#23[e#4, e#5] -> e#7\n\
        sk#50[e#6, e#7] -> e#8\n\
        sk#51[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q2(e#2, e#3)>, <Q21(e#4, e#5)>, <Q212(e#6, e#7)>, <Q2122(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#10[e#2, e#3] -> e#4\n\
        sk#11[e#2, e#3] -> e#5\n\
        sk#22[e#4, e#5] -> e#6\n\
        sk#23[e#4, e#5] -> e#7\n\
        sk#50[e#6, e#7] -> e#8\n\
        sk#51[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q2(e#2, e#3)>, <Q22(e#4, e#5)>, <Q221(e#6, e#7)>, <Q2211(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#10[e#2, e#3] -> e#4\n\
        sk#11[e#2, e#3] -> e#5\n\
        sk#26[e#4, e#5] -> e#6\n\
        sk#27[e#4, e#5] -> e#7\n\
        sk#54[e#6, e#7] -> e#8\n\
        sk#55[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q2(e#2, e#3)>, <Q22(e#4, e#5)>, <Q221(e#6, e#7)>, <Q2212(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#10[e#2, e#3] -> e#4\n\
        sk#11[e#2, e#3] -> e#5\n\
        sk#26[e#4, e#5] -> e#6\n\
        sk#27[e#4, e#5] -> e#7\n\
        sk#54[e#6, e#7] -> e#8\n\
        sk#55[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q2(e#2, e#3)>, <Q22(e#4, e#5)>, <Q222(e#6, e#7)>, <Q2221(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#10[e#2, e#3] -> e#4\n\
        sk#11[e#2, e#3] -> e#5\n\
        sk#26[e#4, e#5] -> e#6\n\
        sk#27[e#4, e#5] -> e#7\n\
        sk#58[e#6, e#7] -> e#8\n\
        sk#59[e#6, e#7] -> e#9\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
        Facts: <Q(e#0, e#1)>, <Q2(e#2, e#3)>, <Q22(e#4, e#5)>, <Q222(e#6, e#7)>, <Q2222(e#8, e#9)>\n\
        'sk#0 -> e#0\n\
        'sk#1 -> e#1\n\
        sk#2[e#0, e#1] -> e#2\n\
        sk#3[e#0, e#1] -> e#3\n\
        sk#10[e#2, e#3] -> e#4\n\
        sk#11[e#2, e#3] -> e#5\n\
        sk#26[e#4, e#5] -> e#6\n\
        sk#27[e#4, e#5] -> e#7\n\
        sk#58[e#6, e#7] -> e#8\n\
        sk#59[e#6, e#7] -> e#9", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy36.raz"))));
        assert_eq!("", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy37.raz"))));
        assert_eq!("Domain: {e#0}\n\
                       Facts: <R(e#0, e#0, e#0)>\n\
                       'sk#0, 'sk#1, 'sk#2 -> e#0", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy38.raz"))));
        assert_eq!("Domain: {e#0, e#1, e#2, e#3, e#4, e#5, e#6}\n\
                       Facts: <Q(e#1)>, <R(e#1, e#6)>\n\
                       'sk#0 -> e#0\n\
                       f[e#0] -> e#1\n\
                       f[e#1] -> e#2\n\
                       f[e#2] -> e#3\n\
                       f[e#3] -> e#4\n\
                       f[e#4] -> e#5\n\
                       f[e#5] -> e#6", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy39.raz"))));
        assert_eq!("Domain: {e#0, e#1, e#2, e#3, e#4}\n\
        Facts: <P(e#1)>, <Q(e#1)>, <R(e#0, e#1)>, <R(e#1, e#3)>, <S(e#4)>\n\
        'sk#0 -> e#0\n\
        f[e#0] -> e#1\n\
        f[e#1] -> e#2\n\
        f[e#2] -> e#3\n\
        sk#1[e#1] -> e#4", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy40.raz"))));
        assert_eq!("Domain: {}\n\
                       Facts: \n", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy41.raz"))));
        assert_eq!("Domain: {e#0}\n\
        Facts: \n\
        'e, 'sk#0, f[e#0, e#0], i[e#0] -> e#0", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy42.raz"))));
        assert_eq!("Domain: {e#0, e#1}\n\
        Facts: <P(e#0)>, <P(e#1)>, <Q(e#0)>, <Q(e#1)>\n\
        'a -> e#0\n\
        'b -> e#1", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy43.raz"))));
        assert_eq!("Domain: {e#0}\n\
        Facts: <P(e#0)>, <Q(e#0)>\n\
        'a -> e#0\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0}\n\
        Facts: <P(e#0)>, <R(e#0)>\n\
        'a -> e#0", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy44.raz"))));
        assert_eq!("Domain: {e#0}\n\
        Facts: <P(e#0)>, <Q(e#0)>\n\
        'a, \'b -> e#0\n\
        -- -- -- -- -- -- -- -- -- --\n\
        Domain: {e#0, e#1}\n\
        Facts: <P(e#0)>, <Q(e#1)>, <R(e#0, e#1)>\n\
        'a -> e#0\n\
        'b -> e#1", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy45.raz"))));
        assert_eq!("Domain: {e#0}\n\
        Facts: <P(e#0)>, <Q(e#0)>, <R(e#0, e#0)>\n\
        'sk#0, 'sk#1 -> e#0", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy46.raz"))));
        assert_eq!("Domain: {e#0}\n\
        Facts: <O(e#0)>, <P(e#0)>, <Q(e#0)>, <R(e#0)>, <S(e#0, e#0, e#0, e#0)>\n\
        'sk#0, 'sk#1, 'sk#2, 'sk#3 -> e#0", print_basic_models(solve_basic(&read_theory_from_file("../theories/core/thy47.raz"))));
    }

    #[test]
    fn test_bounded() {
//        assert_eq!("Domain: {e#0, e#1, e#2, e#3, e#4}\n\
//        Facts: <P(e#0)>, <P(e#1)>, <P(e#2)>, <P(e#3)>, <P(e#4)>\n\
//        'a -> e#0\n\
//        f[e#0] -> e#1\n\
//        f[e#1] -> e#2\n\
//        f[e#2] -> e#3\n\
//        f[e#3] -> e#4", print_basic_models(solve_domain_bounded_basic(&read_theory_from_file("../theories/bounded/thy0.raz"), 5)));
//        assert_eq!("Domain: {e#0, e#1, e#10, e#11, e#12, e#13, e#14, e#15, e#16, e#17, e#18, e#19, e#2, e#3, e#4, e#5, e#6, e#7, e#8, e#9}\n\
//        Facts: <P(e#0)>, <P(e#1)>, <P(e#10)>, <P(e#11)>, <P(e#12)>, <P(e#13)>, <P(e#14)>, <P(e#15)>, <P(e#16)>, <P(e#17)>, <P(e#18)>, <P(e#19)>, <P(e#2)>, <P(e#3)>, <P(e#4)>, <P(e#5)>, <P(e#6)>, <P(e#7)>, <P(e#8)>, <P(e#9)>\n\
//        'a -> e#0\n\
//        f[e#0] -> e#1\n\
//        f[e#1] -> e#2\n\
//        f[e#2] -> e#3\n\
//        f[e#3] -> e#4\n\
//        f[e#4] -> e#5\n\
//        f[e#5] -> e#6\n\
//        f[e#6] -> e#7\n\
//        f[e#7] -> e#8\n\
//        f[e#8] -> e#9\n\
//        f[e#9] -> e#10\n\
//        f[e#10] -> e#11\n\
//        f[e#11] -> e#12\n\
//        f[e#12] -> e#13\n\
//        f[e#13] -> e#14\n\
//        f[e#14] -> e#15\n\
//        f[e#15] -> e#16\n\
//        f[e#16] -> e#17\n\
//        f[e#17] -> e#18\n\
//        f[e#18] -> e#19", print_basic_models(solve_domain_bounded_basic(&read_theory_from_file("../theories/bounded/thy0.raz"), 20)));
//        assert_eq!("Domain: {e#0, e#1, e#2, e#3, e#4}\n\
//        Facts: <P(e#0)>, <P(e#1)>, <P(e#2)>, <P(e#3)>, <P(e#4)>\n\
//        'a -> e#0\n\
//        f[e#0] -> e#1\n\
//        f[e#1] -> e#2\n\
//        f[e#2] -> e#3\n\
//        f[e#3] -> e#4", print_basic_models(solve_domain_bounded_basic(&read_theory_from_file("../theories/bounded/thy2.raz"), 5)));
        assert_eq!(
            r#"Domain: {e#0}
Facts: <P(e#0)>, <Q(e#0)>
'sk#0 -> e#0"#,
            print_basic_models(solve_domain_bounded_basic(&read_theory_from_file("../theories/bounded/thy3.raz"), 5)));
        assert_eq!(
            r#"Domain: {e#0}
Facts: <P(e#0)>, <Q(e#0)>
'a -> e#0
-- -- -- -- -- -- -- -- -- --
Domain: {e#0, e#1}
Facts: <P(e#0)>, <P(e#1)>, <Q(e#1)>
'a -> e#0
sk#0[e#0] -> e#1
-- -- -- -- -- -- -- -- -- --
Domain: {e#0, e#1, e#2}
Facts: <P(e#0)>, <P(e#1)>, <P(e#2)>, <Q(e#2)>
'a -> e#0
sk#0[e#0] -> e#1
sk#0[e#1] -> e#2
-- -- -- -- -- -- -- -- -- --
Domain: {e#0, e#1, e#2, e#3}
Facts: <P(e#0)>, <P(e#1)>, <P(e#2)>, <P(e#3)>, <Q(e#3)>
'a -> e#0
sk#0[e#0] -> e#1
sk#0[e#1] -> e#2
sk#0[e#2] -> e#3
-- -- -- -- -- -- -- -- -- --
Domain: {e#0, e#1, e#2, e#3, e#4}
Facts: <P(e#0)>, <P(e#1)>, <P(e#2)>, <P(e#3)>, <P(e#4)>, <Q(e#4)>
'a -> e#0
sk#0[e#0] -> e#1
sk#0[e#1] -> e#2
sk#0[e#2] -> e#3
sk#0[e#3] -> e#4"#,
            print_basic_models(solve_domain_bounded_basic(&read_theory_from_file("../theories/bounded/thy4.raz"), 5)));
    }
}