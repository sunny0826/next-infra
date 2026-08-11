#![cfg(feature = "typescript-bindings")]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use next_infra_query::dto::{
    BindingCommandResultDto, BindingDto, BindingSnapshotDto, BindingStatusDto, ChangeDto,
    ChangeOriginDto, ChangePageDto, ChangeSubjectDto, ConnectionDto, ConnectionPurgeSummary,
    ConnectionSnapshotDto, ConnectorCoverageDto, ConnectorCoverageLevelDto,
    ConnectorCoverageSnapshotDto, ConnectorHealth, ConnectorHealthCountsDto,
    CreateBindingCommandDto, DisableBindingCommandDto, ErrorEnvelope, EvidenceType, FieldChangeDto,
    Freshness, FreshnessCountsDto, FrontierDirectionDto, HealthSummaryDto, Lifecycle, PageInfo,
    QueryViewState, RelationDto, RelationEvidenceDto, RelationPageDto, ResourceDetailDto,
    ResourceDto, ResourceHealth, ResourceHealthCountsDto, ResourcePageDto, SchemaVersion,
    SnapshotMetadata, SyncCoverageDto, SyncModeDto, SyncRunCountsDto, SyncRunDto, SyncRunErrorDto,
    SyncRunStatusDto, SyncRunWarningDto, SyncStatusDto, SyncTriggerDto, TimelineGroupDto,
    TimelineItemDto, TimelineOriginDto, TimelinePageDto, TimelineVersionLinkDto, TopologyDto,
    TopologyFrontierDto, UpdateBindingCommandDto,
};
use ts_rs::{Config, TS};

const EXPECTED_BINDINGS: [&str; 55] = [
    "BindingCommandResultDto.ts",
    "BindingDto.ts",
    "BindingSnapshotDto.ts",
    "BindingStatusDto.ts",
    "ChangeDto.ts",
    "ChangeOriginDto.ts",
    "ChangePageDto.ts",
    "ChangeSubjectDto.ts",
    "ConnectionDto.ts",
    "ConnectionPurgeSummary.ts",
    "ConnectionSnapshotDto.ts",
    "ConnectorCoverageDto.ts",
    "ConnectorCoverageLevelDto.ts",
    "ConnectorCoverageSnapshotDto.ts",
    "ConnectorHealth.ts",
    "ConnectorHealthCountsDto.ts",
    "CreateBindingCommandDto.ts",
    "DisableBindingCommandDto.ts",
    "ErrorEnvelope.ts",
    "EvidenceType.ts",
    "FieldChangeDto.ts",
    "Freshness.ts",
    "FreshnessCountsDto.ts",
    "FrontierDirectionDto.ts",
    "HealthSummaryDto.ts",
    "Lifecycle.ts",
    "PageInfo.ts",
    "QueryViewState.ts",
    "RelationDto.ts",
    "RelationEvidenceDto.ts",
    "RelationPageDto.ts",
    "ResourceDetailDto.ts",
    "ResourceDto.ts",
    "ResourceHealth.ts",
    "ResourceHealthCountsDto.ts",
    "ResourcePageDto.ts",
    "SchemaVersion.ts",
    "SnapshotMetadata.ts",
    "SyncCoverageDto.ts",
    "SyncModeDto.ts",
    "SyncRunCountsDto.ts",
    "SyncRunDto.ts",
    "SyncRunErrorDto.ts",
    "SyncRunStatusDto.ts",
    "SyncRunWarningDto.ts",
    "SyncStatusDto.ts",
    "SyncTriggerDto.ts",
    "TimelineGroupDto.ts",
    "TimelineItemDto.ts",
    "TimelineOriginDto.ts",
    "TimelinePageDto.ts",
    "TimelineVersionLinkDto.ts",
    "TopologyDto.ts",
    "TopologyFrontierDto.ts",
    "UpdateBindingCommandDto.ts",
];

fn output_directory() -> PathBuf {
    if let Some(path) = std::env::var_os("NEXT_INFRA_QUERY_BINDINGS_DIR") {
        return PathBuf::from(path);
    }

    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/src/generated/query")
}

fn reset_output_directory(path: &Path) {
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("query")
    );

    if path.exists() {
        fs::remove_dir_all(path).expect("failed to remove previous query bindings");
    }
    fs::create_dir_all(path).expect("failed to create query bindings directory");
}

#[test]
fn export_types() {
    let output = output_directory();
    reset_output_directory(&output);
    let config = Config::new().with_out_dir(&output);

    SchemaVersion::export_all(&config).expect("failed to export schema version binding");
    SnapshotMetadata::export_all(&config).expect("failed to export snapshot metadata binding");
    PageInfo::export_all(&config).expect("failed to export page info binding");
    ErrorEnvelope::export_all(&config).expect("failed to export error envelope binding");
    Lifecycle::export_all(&config).expect("failed to export lifecycle binding");
    ResourceHealth::export_all(&config).expect("failed to export resource health binding");
    Freshness::export_all(&config).expect("failed to export freshness binding");
    ResourceDto::export_all(&config).expect("failed to export resource binding");
    EvidenceType::export_all(&config).expect("failed to export evidence type binding");
    RelationDto::export_all(&config).expect("failed to export relation binding");
    ConnectorHealth::export_all(&config).expect("failed to export connector health binding");
    ConnectionDto::export_all(&config).expect("failed to export connection binding");
    ConnectionSnapshotDto::export_all(&config)
        .expect("failed to export connection snapshot binding");
    QueryViewState::export_all(&config).expect("failed to export query view state binding");
    SyncModeDto::export_all(&config).expect("failed to export sync mode binding");
    SyncTriggerDto::export_all(&config).expect("failed to export sync trigger binding");
    SyncRunStatusDto::export_all(&config).expect("failed to export sync status binding");
    SyncCoverageDto::export_all(&config).expect("failed to export sync coverage binding");
    ConnectorCoverageLevelDto::export_all(&config)
        .expect("failed to export connector coverage level binding");
    ConnectorCoverageDto::export_all(&config).expect("failed to export connector coverage binding");
    SyncRunCountsDto::export_all(&config).expect("failed to export sync counts binding");
    SyncRunErrorDto::export_all(&config).expect("failed to export sync error binding");
    SyncRunWarningDto::export_all(&config).expect("failed to export sync warning binding");
    SyncRunDto::export_all(&config).expect("failed to export sync run binding");
    ChangeSubjectDto::export_all(&config).expect("failed to export change subject binding");
    FieldChangeDto::export_all(&config).expect("failed to export field change binding");
    ChangeOriginDto::export_all(&config).expect("failed to export change origin binding");
    ChangeDto::export_all(&config).expect("failed to export change binding");
    RelationEvidenceDto::export_all(&config).expect("failed to export relation evidence binding");
    ResourcePageDto::export_all(&config).expect("failed to export resource page binding");
    RelationPageDto::export_all(&config).expect("failed to export relation page binding");
    ResourceDetailDto::export_all(&config).expect("failed to export resource detail binding");
    FrontierDirectionDto::export_all(&config).expect("failed to export frontier direction binding");
    TopologyFrontierDto::export_all(&config).expect("failed to export topology frontier binding");
    TopologyDto::export_all(&config).expect("failed to export topology binding");
    BindingStatusDto::export_all(&config).expect("failed to export binding status binding");
    BindingDto::export_all(&config).expect("failed to export binding binding");
    CreateBindingCommandDto::export_all(&config)
        .expect("failed to export create binding command binding");
    UpdateBindingCommandDto::export_all(&config)
        .expect("failed to export update binding command binding");
    DisableBindingCommandDto::export_all(&config)
        .expect("failed to export disable binding command binding");
    BindingCommandResultDto::export_all(&config)
        .expect("failed to export binding command result binding");
    BindingSnapshotDto::export_all(&config).expect("failed to export binding snapshot binding");
    TimelineOriginDto::export_all(&config).expect("failed to export timeline origin binding");
    TimelineVersionLinkDto::export_all(&config)
        .expect("failed to export timeline version link binding");
    TimelineItemDto::export_all(&config).expect("failed to export timeline item binding");
    TimelineGroupDto::export_all(&config).expect("failed to export timeline group binding");
    TimelinePageDto::export_all(&config).expect("failed to export timeline page binding");
    ResourceHealthCountsDto::export_all(&config)
        .expect("failed to export resource health counts binding");
    FreshnessCountsDto::export_all(&config).expect("failed to export freshness counts binding");
    ConnectorHealthCountsDto::export_all(&config)
        .expect("failed to export connector health counts binding");
    HealthSummaryDto::export_all(&config).expect("failed to export health summary binding");
    ChangePageDto::export_all(&config).expect("failed to export change page binding");
    SyncStatusDto::export_all(&config).expect("failed to export sync status binding");
    ConnectorCoverageSnapshotDto::export_all(&config)
        .expect("failed to export connector coverage snapshot binding");
    ConnectionPurgeSummary::export_all(&config)
        .expect("failed to export connection purge summary binding");

    let mut generated = BTreeSet::new();
    for entry in fs::read_dir(&output).expect("failed to read generated query bindings") {
        let path = entry
            .expect("failed to read generated binding entry")
            .path();
        generated.insert(
            path.file_name()
                .and_then(|name| name.to_str())
                .expect("generated binding filename must be UTF-8")
                .to_owned(),
        );
        let contents = fs::read_to_string(&path).expect("failed to read generated binding");
        assert!(
            contents.starts_with("// This file was generated by [ts-rs]")
                && contents.contains("Do not edit this file manually."),
            "missing generated-file notice in {}",
            path.display()
        );
    }

    let expected = EXPECTED_BINDINGS
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    assert_eq!(generated, expected, "generated query binding set drifted");

    let page_info = fs::read_to_string(output.join("PageInfo.ts"))
        .expect("failed to read generated PageInfo binding");
    assert!(page_info.contains("readonly __opaqueCursor: unique symbol"));
}
