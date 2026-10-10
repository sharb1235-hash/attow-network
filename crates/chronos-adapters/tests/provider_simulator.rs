//! Real loopback server persistence and independently controlled provider time.
use attow_adapters::provider_simulator::{SimulatorConfig, SimulatorServer};
use chronos_codec::ObjectId;

#[test]
fn holds_reserve_inventory_until_expiry_and_survive_server_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("provider.sqlite");
    let config = SimulatorConfig {
        inventory: 2,
        hold_ttl: 5,
        ..SimulatorConfig::default()
    };
    {
        let server = SimulatorServer::start(&path, &config)?;
        let controller = server.controller();
        assert_eq!(controller.stats()?.inventory, 2);
        controller.hold(ObjectId([1; 32]), 2)?;
        assert_eq!(controller.stats()?.held, 2);
        assert!(controller.hold(ObjectId([2; 32]), 1).is_err());
    }
    let server = SimulatorServer::start(&path, &config)?;
    let controller = server.controller();
    assert_eq!(controller.stats()?.held, 2);
    controller.advance_ticks(5)?;
    assert_eq!(controller.stats()?.held, 0);
    controller.hold(ObjectId([2; 32]), 1)?;
    assert_eq!(controller.stats()?.held, 1);
    assert_eq!(controller.stats()?.accepted, 0);
    Ok(())
}

#[test]
fn existing_provider_state_cannot_be_reopened_with_changed_contract_parameters()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("provider.sqlite");
    let config = SimulatorConfig::default();
    drop(SimulatorServer::start(&path, &config)?);
    let changed = SimulatorConfig {
        idempotency_retention: config.idempotency_retention + 1,
        ..config
    };
    assert!(SimulatorServer::start(&path, &changed).is_err());
    Ok(())
}
