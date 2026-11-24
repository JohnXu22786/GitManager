use super::*;
fn term(n: i64) -> PropertyTerm { PropertyTerm::Literal { value_type: Type::Integer, value: DataValue::Integer { value: n } } }
fn count_at(point: &str, observable: &str) -> PropertyTerm { PropertyTerm::Observed { point: point.into(), observable: observable.into(), value_type: Type::Integer } }
fn count() -> PropertyTerm { count_at("result", "policy") }
fn classify(p: &PropertyPredicate, cap: i64) -> Result<Vec<(i64, i64)>> {
    predicate_intervals(p, &mut |point, observable| {
        if point == "result" && observable == "policy" { Ok(cap) } else { Err(invalid("unqualified test leaf")) }
    }, &mut |_| Ok(())).map(|(_, _, intervals)| intervals)
}
#[test]
fn policy_interval_boundaries_are_exact_for_reversed_atoms_and_i64_extremes() {
    for (predicate, expected) in [
        (PropertyPredicate::Less { left: count(), right: term(2) }, vec![(0,1)]),
        (PropertyPredicate::Less { left: term(2), right: count() }, vec![(3,3)]),
        (PropertyPredicate::Equal { left: term(2), right: count() }, vec![(2,2)]),
        (PropertyPredicate::Equal { left: count(), right: term(0) }, vec![(0,0)]),
    ] { assert_eq!(classify(&predicate, 3).unwrap(), expected); }
    for n in [i64::MIN, i64::MAX] {
        assert!(classify(&PropertyPredicate::Equal { left: count(), right: term(n) },3).is_err());
        assert!(classify(&PropertyPredicate::Less { left: count(), right: term(n) },3).is_err());
        assert!(classify(&PropertyPredicate::Less { left: term(n), right: count() },3).is_err());
    }
    assert!(classify(&PropertyPredicate::Less { left: count(), right: term(2) },1).is_err());
    assert!(classify(&PropertyPredicate::Equal { left: count(), right: term(0) },0).is_err());
}
#[test]
fn policy_boolean_interval_sets_reject_empty_full_and_every_hidden_foreign_branch() {
    let eq = |n| PropertyPredicate::Equal { left: count(), right: term(n) };
    assert_eq!(classify(&PropertyPredicate::Not { value: Box::new(eq(1)) },3).unwrap(), vec![(0,0),(2,3)]);
    assert_eq!(classify(&PropertyPredicate::Or { values: vec![eq(2),eq(1),eq(1)] },3).unwrap(),vec![(1,2)]);
    assert_eq!(classify(&PropertyPredicate::And { values: vec![PropertyPredicate::Less { left: count(), right: term(3) },PropertyPredicate::Not { value: Box::new(eq(0)) }] },3).unwrap(),vec![(1,2)]);
    for p in [PropertyPredicate::And { values: vec![] },PropertyPredicate::Or { values: vec![] },PropertyPredicate::And { values: vec![eq(0),eq(1)] },PropertyPredicate::Or { values: (0..4).map(eq).collect() }] { assert!(classify(&p,3).is_err()); }
    for foreign in [count_at("other-point","policy"),count_at("result","common"),PropertyTerm::Count { value: Box::new(count()) },PropertyTerm::OutputCount { point:"result".into(),output:"output".into() },PropertyTerm::ViewColumn { point:"result".into(),column:"policy".into(),value_type:Type::Integer }] {
        let p = PropertyPredicate::Or { values: vec![PropertyPredicate::Less { left: count(), right: term(2) },PropertyPredicate::Equal { left:foreign,right:term(0) }] };
        let error = classify(&p,3).unwrap_err();
        assert!(matches!(error, DecisionError::Invalid(ref text) if text == "unqualified test leaf" || text == "owned retirement needs a direct integer policy count and integer literal"));
    }
    assert!(classify(&PropertyPredicate::Equal { left: count(), right: count() },3).is_err());
    assert!(classify(&PropertyPredicate::Equal { left: term(1), right: term(1) },3).is_err());
}
#[test]
fn predicate_refusal_stops_at_the_first_real_charge() {
    let p = PropertyPredicate::Or { values: vec![PropertyPredicate::Equal { left:count(),right:term(0) },PropertyPredicate::Equal { left:count(),right:term(2) }] };
    let mut calls = 0;
    predicate_intervals(&p,&mut |_,_| Ok(3),&mut |_| { calls += 1; Ok(()) }).unwrap();
    assert!(calls > 0);
    for fail in 0..calls {
        let mut reached = 0;
        let result = predicate_intervals(&p,&mut |_,_| Ok(3),&mut |_| {
            let here = reached; reached += 1;
            if here == fail { Err(invalid("literal refusal sentinel")) } else { Ok(()) }
        });
        assert!(matches!(result,Err(DecisionError::Invalid(s)) if s == "literal refusal sentinel"));
        assert_eq!(reached,fail+1);
    }
}


#[test]
fn transition_callback_errors_preserve_store_and_runtime_causes() {
    use super::super::owned_transition::into_store_error;
    let reason = String::from("Common factual admission: budget: cumulative proof/initialization budget exhausted");
    let allocation = reason.as_ptr();
    match into_store_error(DecisionError::Store(StoreError::Invalid(reason))) {
        StoreError::Invalid(actual) => {
            assert_eq!(actual, "Common factual admission: budget: cumulative proof/initialization budget exhausted");
            assert_eq!(actual.as_ptr(), allocation);
        }
        other => panic!("lost admission cause: {other:?}"),
    }
    assert!(matches!(into_store_error(DecisionError::Store(StoreError::Conflict("exact conflict".into()))), StoreError::Conflict(text) if text == "exact conflict"));
    assert!(matches!(into_store_error(DecisionError::Store(StoreError::Io(std::io::Error::from_raw_os_error(22)))), StoreError::Io(error) if error.raw_os_error() == Some(22)));
    for cause in [
        AdapterError::Invalid(ContractError("exact contract cause".into())),
        AdapterError::Unsupported("exact unsupported cause".into()),
        AdapterError::Stale("exact stale cause".into()),
        AdapterError::Cancelled,
        AdapterError::BudgetExhausted("exact runtime fuel cause".into()),
        AdapterError::Failed("exact runtime failure".into()),
    ] {
        for input in [DecisionError::Runtime(cause.clone()), DecisionError::Store(StoreError::Runtime(cause.clone()))] {
            assert!(matches!(into_store_error(input), StoreError::Runtime(actual) if actual == cause));
        }
    }
    for input in [DecisionError::Invalid("decision invalid".into()), DecisionError::Unverified("decision unverified".into()), DecisionError::Archive("decision archive".into())] {
        let expected = input.to_string();
        assert!(matches!(into_store_error(input), StoreError::Invalid(actual) if actual == expected));
    }
}
