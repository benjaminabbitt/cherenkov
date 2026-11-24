//! A rejection and its typed refusal, both ways.

use angzarr_embedded::markup::{Failed, Refuses, TypedRefusal};

use super::*;
use crate::Mutant;
use crate::{
    AlreadyPlanned, MutantRejection, NoMutant, NoRun, PlanMutant, PlanRun, PlannedForRun, Run,
    RunRejection, mutant, run,
};

/// The refusal message of every rejection of the run, with the rejection it
/// must read back as.
fn run_table() -> Vec<(TypedRefusal, RunRejection)> {
    vec![(
        TypedRefusal::of(&AlreadyPlanned {}),
        RunRejection::AlreadyPlanned,
    )]
}

/// The refusal message of every rejection of a mutant, with the rejection it
/// must read back as.
fn mutant_table() -> Vec<(TypedRefusal, MutantRejection)> {
    vec![
        (TypedRefusal::of(&NoRun {}), MutantRejection::NoRun),
        (TypedRefusal::of(&NoMutant {}), MutantRejection::NoMutant),
        (
            TypedRefusal::of(&PlannedForRun {}),
            MutantRejection::PlannedForRun,
        ),
    ]
}

#[test]
fn every_refusal_reads_back_as_its_rejection_by_its_type() {
    for (refusal, rejection) in run_table() {
        assert_eq!(RunRejection::try_from(&refusal), Ok(rejection));
    }
    for (refusal, rejection) in mutant_table() {
        assert_eq!(MutantRejection::try_from(&refusal), Ok(rejection));
    }
}

#[test]
fn a_message_that_is_no_refusal_of_ours_is_none_of_ours() {
    let other = TypedRefusal::of(&PlanRun::default());
    assert_eq!(RunRejection::try_from(&other), Err(NotOurs));
    assert_eq!(MutantRejection::try_from(&other), Err(NotOurs));
    // A refusal of the mutant's is none of the run's, and the other way.
    assert_eq!(
        RunRejection::try_from(&TypedRefusal::of(&NoRun {})),
        Err(NotOurs)
    );
    assert_eq!(
        MutantRejection::try_from(&TypedRefusal::of(&AlreadyPlanned {})),
        Err(NotOurs)
    );
}

/// What `PlanRun` refuses with for `rejection`.
fn plan_run_refuses(rejection: RunRejection) -> Failed<Refuses<Run, PlanRun>> {
    refuse!(
        RunRejection,
        rejection,
        Refuses<Run, PlanRun>,
        run,
        [AlreadyPlanned => AlreadyPlanned]
    )
}

/// What `PlanMutant` refuses with for `rejection`.
fn plan_mutant_refuses(rejection: MutantRejection) -> Failed<Refuses<Mutant, PlanMutant>> {
    refuse!(
        MutantRejection,
        rejection,
        Refuses<Mutant, PlanMutant>,
        mutant,
        [NoRun => NoRun, NoMutant => NoMutant, PlannedForRun => PlannedForRun]
    )
}

#[test]
fn a_handler_refuses_with_the_message_of_the_rejection_under_its_declared_status() {
    let Failed::Rejected { rejection, status } = plan_run_refuses(RunRejection::AlreadyPlanned)
    else {
        panic!("a refusal");
    };
    assert_eq!(
        TypedRefusal::from(rejection),
        TypedRefusal::of(&AlreadyPlanned {})
    );
    assert_eq!(status, <AlreadyPlanned as run::Refusal>::REFUSAL_STATUS);

    for (rejection, message, declared) in [
        (
            MutantRejection::NoRun,
            TypedRefusal::of(&NoRun {}),
            <NoRun as mutant::Refusal>::REFUSAL_STATUS,
        ),
        (
            MutantRejection::NoMutant,
            TypedRefusal::of(&NoMutant {}),
            <NoMutant as mutant::Refusal>::REFUSAL_STATUS,
        ),
        (
            MutantRejection::PlannedForRun,
            TypedRefusal::of(&PlannedForRun {}),
            <PlannedForRun as mutant::Refusal>::REFUSAL_STATUS,
        ),
    ] {
        let Failed::Rejected { rejection, status } = plan_mutant_refuses(rejection) else {
            panic!("a refusal");
        };
        assert_eq!(TypedRefusal::from(rejection), message);
        assert_eq!(status, declared);
    }
}

#[test]
fn each_refusal_travels_under_the_status_its_message_declares() {
    use angzarr_embedded::markup::BusinessStatus;
    assert_eq!(
        <AlreadyPlanned as run::Refusal>::REFUSAL_STATUS,
        BusinessStatus::AlreadyExists
    );
    assert_eq!(
        <PlannedForRun as mutant::Refusal>::REFUSAL_STATUS,
        BusinessStatus::AlreadyExists
    );
    assert_eq!(
        <NoRun as mutant::Refusal>::REFUSAL_STATUS,
        BusinessStatus::InvalidArgument
    );
    assert_eq!(
        <NoMutant as mutant::Refusal>::REFUSAL_STATUS,
        BusinessStatus::InvalidArgument
    );
}

#[test]
fn a_rejection_the_command_lists_no_message_for_is_the_handlers_bug() {
    assert!(matches!(
        plan_mutant_refuses_only_planned(MutantRejection::NoRun),
        Failed::Bug(_)
    ));
}

/// A handler that lists `PlannedForRun` alone.
fn plan_mutant_refuses_only_planned(
    rejection: MutantRejection,
) -> Failed<Refuses<Mutant, PlanMutant>> {
    refuse!(
        MutantRejection,
        rejection,
        Refuses<Mutant, PlanMutant>,
        mutant,
        [PlannedForRun => PlannedForRun]
    )
}
