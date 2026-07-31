pub use super::_entities::access_key_permissions::{ActiveModel, Column, Entity, Model};

use loco_rs::prelude::*;

// sea-orm-codegen 2.0 no longer emits this in the generated entity; loco's
// convention is to implement it in the sibling model module.
#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {}
