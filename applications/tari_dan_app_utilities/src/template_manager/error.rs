//   Copyright 2023. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use tari_dan_common_types::optional::IsNotFoundError;
use tari_dan_core::storage::StorageError;
use tari_dan_engine::packager::PackageError;
use tari_dan_storage_sqlite::error::SqliteStorageError;
use tari_template_lib::models::TemplateAddress;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TemplateManagerError {
    #[error("The template in the base layer is invalid")]
    InvalidBaseLayerTemplate,
    #[error("Internal service channel closed unexpectedly")]
    ChannelClosed,
    #[error("Storage error: {0}")]
    StorageError(#[from] StorageError),
    #[error("Storage error: {0}")]
    SqliteStorageError(#[from] SqliteStorageError),
    #[error("Template not found: {address}")]
    TemplateNotFound { address: TemplateAddress },
    #[error("The template is unavailable for use")]
    TemplateUnavailable,
    #[error(transparent)]
    PackageError(#[from] PackageError),
}

impl IsNotFoundError for TemplateManagerError {
    fn is_not_found_error(&self) -> bool {
        matches!(self, Self::TemplateNotFound { .. })
    }
}
