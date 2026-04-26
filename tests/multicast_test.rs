use aex::connection::multicast::{MulticastGroup, MulticastManager, MulticastScope};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[tokio::test]
async fn test_multicast_manager_new() {
    let manager = MulticastManager::new();
    let groups = manager.all_groups().await;
    assert!(groups.is_empty());
}

#[tokio::test]
async fn test_multicast_manager_create_and_get_group() {
    let manager = MulticastManager::new();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 254)), 8080);

    let group = manager.create_group(addr).await;
    assert_eq!(group.addr, addr);

    let retrieved = manager.get_group(&addr).await;
    assert!(retrieved.is_some());
}

#[tokio::test]
async fn test_multicast_manager_remove_group() {
    let manager = MulticastManager::new();
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 254)), 8080);

    manager.create_group(addr).await;
    manager.remove_group(&addr).await;

    let groups = manager.all_groups().await;
    assert!(groups.is_empty());
}

#[tokio::test]
async fn test_multicast_group_new() {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 254)), 8080);
    let group = MulticastGroup::new(addr);
    assert_eq!(group.addr, addr);
    assert_eq!(group.scope, MulticastScope::SiteLocal);
}

#[tokio::test]
async fn test_multicast_group_new_site_local() {
    let group = MulticastGroup::new_site_local(8080);
    assert_eq!(group.addr.port(), 8080);
}

#[tokio::test]
async fn test_multicast_group_join_leave() {
    let group = MulticastGroup::new_site_local(8080);
    let peer = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9000);

    group.join(peer).await;
    assert!(group.is_member(&peer).await);
    assert_eq!(group.member_count().await, 1);

    group.leave(&peer).await;
    assert!(!group.is_member(&peer).await);
}

#[tokio::test]
async fn test_multicast_members() {
    let group = MulticastGroup::new_site_local(8080);
    let peer1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9000);
    let peer2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)), 9000);

    group.join(peer1).await;
    group.join(peer2).await;

    let members = group.members().await;
    assert_eq!(members.len(), 2);
}

#[test]
fn test_multicast_scope_from_addr() {
    let local = Ipv4Addr::new(224, 0, 0, 1);
    assert_eq!(
        MulticastScope::from_addr(&local),
        Some(MulticastScope::Local)
    );

    let global = Ipv4Addr::new(225, 0, 0, 1);
    assert_eq!(
        MulticastScope::from_addr(&global),
        Some(MulticastScope::Global)
    );

    let admin = Ipv4Addr::new(239, 0, 0, 1);
    assert_eq!(
        MulticastScope::from_addr(&admin),
        Some(MulticastScope::Admin)
    );

    let site_local = Ipv4Addr::new(239, 255, 0, 1);
    assert_eq!(
        MulticastScope::from_addr(&site_local),
        Some(MulticastScope::SiteLocal)
    );

    let org = Ipv4Addr::new(239, 192, 0, 1);
    assert_eq!(
        MulticastScope::from_addr(&org),
        Some(MulticastScope::Organization)
    );
}

#[test]
fn test_multicast_scope_is_admin_local() {
    assert!(MulticastScope::Local.is_admin_local());
    assert!(MulticastScope::Admin.is_admin_local());
    assert!(!MulticastScope::SiteLocal.is_admin_local());
    assert!(!MulticastScope::Organization.is_admin_local());
    assert!(!MulticastScope::Global.is_admin_local());
}

#[test]
fn test_multicast_scope_address_range() {
    let (start, end) = MulticastScope::Local.address_range();
    assert_eq!(start, Ipv4Addr::new(224, 0, 0, 0));
    assert_eq!(end, Ipv4Addr::new(224, 0, 0, 255));
}
