//! Pagination for list endpoints: `?page=1&page_size=50` → `{items,total,page,page_size}`.

use serde::{Deserialize, Serialize};

pub const DEFAULT_PAGE_SIZE: u32 = 50;
pub const MAX_PAGE_SIZE: u32 = 200;

/// Query parameters. Out-of-range values are clamped rather than rejected (friendlier for UIs).
#[derive(Debug, Clone, Copy, Deserialize, utoipa::IntoParams)]
pub struct PageParams {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

impl Default for PageParams {
    fn default() -> Self {
        Self {
            page: Some(1),
            page_size: Some(DEFAULT_PAGE_SIZE),
        }
    }
}

impl PageParams {
    pub fn page(&self) -> u32 {
        self.page.unwrap_or(1).max(1)
    }

    pub fn page_size(&self) -> u32 {
        self.page_size
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE)
    }

    /// SQL `LIMIT`.
    pub fn limit(&self) -> i64 {
        i64::from(self.page_size())
    }

    /// SQL `OFFSET`.
    pub fn offset(&self) -> i64 {
        i64::from(self.page() - 1) * self.limit()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub page: u32,
    pub page_size: u32,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, total: i64, params: &PageParams) -> Self {
        Self {
            items,
            total,
            page: params.page(),
            page_size: params.page_size(),
        }
    }

    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Page<U> {
        Page {
            items: self.items.into_iter().map(f).collect(),
            total: self.total,
            page: self.page,
            page_size: self.page_size,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let p = PageParams {
            page: None,
            page_size: None,
        };
        assert_eq!((p.page(), p.page_size(), p.limit(), p.offset()), (1, 50, 50, 0));
    }

    #[test]
    fn clamps_and_offsets() {
        let p = PageParams {
            page: Some(3),
            page_size: Some(1000),
        };
        assert_eq!(p.page_size(), MAX_PAGE_SIZE);
        assert_eq!(p.offset(), 400);
        let p = PageParams {
            page: Some(0),
            page_size: Some(0),
        };
        assert_eq!((p.page(), p.page_size()), (1, 1));
    }

    #[test]
    fn page_map() {
        let p = Page::new(vec![1, 2], 10, &PageParams::default()).map(|x| x * 10);
        assert_eq!(p.items, vec![10, 20]);
        assert_eq!(p.total, 10);
    }
}
