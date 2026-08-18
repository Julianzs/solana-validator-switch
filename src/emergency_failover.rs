use anyhow::{anyhow, Result};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;

use crate::alert::AlertManager;
use crate::commands::switch::{FailoverMode, SwitchManager};
use crate::ssh::AsyncSshPool;
use crate::types::{NodeWithStatus, ValidatorPair};

pub struct EmergencyFailover {
    active_node: NodeWithStatus,
    standby_node: NodeWithStatus,
    validator_pair: ValidatorPair,
    ssh_pool: Arc<AsyncSshPool>,
    detected_ssh_keys: std::collections::HashMap<String, String>,
    alert_manager: AlertManager,
    // Track results
    primary_switch_success: bool,
    tower_copy_success: bool,
    standby_switch_success: bool,
    total_time: Option<Duration>,
    mode: FailoverMode,
}

impl EmergencyFailover {
    pub fn new(
        active_node: NodeWithStatus,
        standby_node: NodeWithStatus,
        validator_pair: ValidatorPair,
        ssh_pool: Arc<AsyncSshPool>,
        detected_ssh_keys: std::collections::HashMap<String, String>,
        alert_manager: AlertManager,
        mode: FailoverMode,
    ) -> Self {
        Self {
            active_node,
            standby_node,
            validator_pair,
            ssh_pool,
            detected_ssh_keys,
            alert_manager,
            primary_switch_success: false,
            tower_copy_success: false,
            standby_switch_success: false,
            total_time: None,
            mode,
        }
    }

    pub async fn execute_emergency_takeover(&mut self) -> Result<()> {
        let start_time = Instant::now();

        // Log the emergency takeover
        eprintln!("🚨 EMERGENCY TAKEOVER INITIATED");
        eprintln!(
            "   Active node ({}) not voting, attempting failover to standby ({})",
            self.active_node.node.label, self.standby_node.node.label
        );

        // Create switch manager for the operations
        let mut switch_manager = SwitchManager::new(
            self.active_node.clone(),
            self.standby_node.clone(),
            self.validator_pair.clone(),
            self.ssh_pool.clone(),
            self.detected_ssh_keys.clone(),
        );

        std::env::set_var("SVS_SILENT_MODE", "1");
        let plan = self.mode.step_plan();

        if plan.demote_source {
            eprintln!("📤 Switching primary to unfunded...");
        let primary_result = match timeout(
                Duration::from_secs(10),
            switch_manager.switch_primary_to_unfunded(false),
        )
        .await
        {
            Ok(Ok(_)) => {
                eprintln!("   ✅ Primary switched to unfunded successfully");
                Ok(())
            }
            Ok(Err(e)) => {
                eprintln!("   ⚠️  Failed to switch primary: {}", e);
                Err(e)
            }
            Err(_) => {
                eprintln!("   ⚠️  Switch primary timed out");
                Err(anyhow!("Operation timed out"))
            }
        };
        self.primary_switch_success = primary_result.is_ok();
        } else {
            eprintln!(
                "   ⚠️  Primary is unreachable and confirmed delinquent; skipping primary demotion"
            );
        }

        if plan.transfer_tower {
        eprintln!("📤 Copying tower file...");
        let tower_result = match timeout(
                Duration::from_secs(10),
            switch_manager.transfer_tower_file(false),
        )
        .await
        {
            Ok(Ok(_)) => {
                eprintln!("   ✅ Tower file copied successfully");
                Ok(())
            }
            Ok(Err(e)) => {
                eprintln!("   ⚠️  Failed to copy tower: {}", e);
                Err(e)
            }
            Err(_) => {
                eprintln!("   ⚠️  Tower copy timed out");
                Err(anyhow!("Operation timed out"))
            }
        };
        self.tower_copy_success = tower_result.is_ok();
        } else {
            eprintln!("   ⚠️  Tower copy skipped because the primary is unreachable");
        }

        // Standby promotion is required in every mode.
        debug_assert!(plan.promote_standby);
        eprintln!("🚀 Switching standby to funded identity...");
        match switch_manager.switch_backup_to_funded(false).await {
            Ok(_) => {
                self.standby_switch_success = true;
                eprintln!("   ✅ Standby switched to funded identity successfully");
            }
            Err(e) => {
                eprintln!("   ❌ CRITICAL: Failed to switch standby to funded: {}", e);
                self.total_time = Some(start_time.elapsed());

                // Send failure notification
                let _ = self
                    .alert_manager
                    .send_emergency_takeover_alert(
                        &self.validator_pair.identity_pubkey,
                        &self.active_node.node.label,
                        &self.standby_node.node.label,
                        self.primary_switch_success,
                        self.tower_copy_success,
                        false, // standby switch failed
                        self.total_time.unwrap(),
                        Some(&format!("Failed to activate standby: {}", e)),
                    )
                    .await;

                return Err(anyhow!(
                    "Emergency takeover failed: could not activate standby node"
                ));
            }
        }

        self.total_time = Some(start_time.elapsed());

        // Send success notification
        let _ = self
            .alert_manager
            .send_emergency_takeover_alert(
                &self.validator_pair.identity_pubkey,
                &self.active_node.node.label,
                &self.standby_node.node.label,
                self.primary_switch_success,
                self.tower_copy_success,
                self.standby_switch_success,
                self.total_time.unwrap(),
                None,
            )
            .await;

        eprintln!(
            "\n✅ Emergency takeover completed in {:?}",
            self.total_time.unwrap()
        );
        eprintln!(
            "   Primary → Unfunded: {}",
            if self.primary_switch_success {
                "✅"
            } else {
                "❌"
            }
        );
        eprintln!(
            "   Tower Copy: {}",
            if self.tower_copy_success {
                "✅"
            } else {
                "❌"
            }
        );
        eprintln!("   Standby → Funded: ✅");

        Ok(())
    }
}
