use qbot::models::{Format, Session, SessionPlayer, SessionStatus};
use qbot::Player;
use serenity::all::UserId;

fn pool(ids: &[u64]) -> Vec<SessionPlayer> {
  ids.iter().map(|id| SessionPlayer::add(Player::add(UserId::new(*id), format!("p{id}"), 0, None, None))).collect()
}

fn ids(session: &Session) -> Vec<u64> {
  session.pool.iter().map(|p| p.player.user_id.get()).collect()
}

fn fmt_with(sessions: Vec<Session>) -> Format {
  let mut fmt = Format::new(0, "4s".into(), 8);
  fmt.sessions = sessions;
  fmt
}

#[test]
fn splits_overflow_into_next_queue() {
  let mut fmt = fmt_with(vec![Session::new(SessionStatus::Idle, pool(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]))]);
  assert!(fmt.pack_idle_sessions());
  assert_eq!(fmt.sessions.len(), 2);
  assert_eq!(ids(&fmt.sessions[0]), vec![1, 2, 3, 4, 5, 6, 7, 8]);
  assert_eq!(ids(&fmt.sessions[1]), vec![9, 10, 11]);
}

#[test]
fn merges_fragmented_queues() {
  let mut fmt = fmt_with(vec![Session::new(SessionStatus::Idle, pool(&[1, 2, 3, 4, 5, 6, 7])), Session::new(SessionStatus::Idle, pool(&[8, 9, 10, 11]))]);
  assert!(fmt.pack_idle_sessions());
  assert_eq!(fmt.sessions.len(), 2);
  assert_eq!(ids(&fmt.sessions[0]), vec![1, 2, 3, 4, 5, 6, 7, 8]);
  assert_eq!(ids(&fmt.sessions[1]), vec![9, 10, 11]);
}

#[test]
fn drops_emptied_queues_but_keeps_one() {
  let mut fmt = fmt_with(vec![Session::new(SessionStatus::Idle, pool(&[1, 2])), Session::new(SessionStatus::Idle, pool(&[3]))]);
  assert!(fmt.pack_idle_sessions());
  assert_eq!(fmt.sessions.len(), 1);
  assert_eq!(ids(&fmt.sessions[0]), vec![1, 2, 3]);
}

#[test]
fn leaves_active_sessions_alone() {
  let mut fmt = fmt_with(vec![
    Session::new(SessionStatus::Live, pool(&[1, 2, 3, 4, 5, 6, 7, 8])),
    Session::new(SessionStatus::Idle, pool(&[9, 10, 11, 12, 13, 14, 15, 16, 17])),
  ]);
  assert!(fmt.pack_idle_sessions());
  assert_eq!(fmt.sessions.len(), 3);
  assert_eq!(fmt.sessions[0].status, SessionStatus::Live);
  assert_eq!(ids(&fmt.sessions[0]), vec![1, 2, 3, 4, 5, 6, 7, 8]);
  assert_eq!(ids(&fmt.sessions[1]), vec![9, 10, 11, 12, 13, 14, 15, 16]);
  assert_eq!(ids(&fmt.sessions[2]), vec![17]);
}

#[test]
fn stable_when_already_packed() {
  let mut fmt = fmt_with(vec![Session::new(SessionStatus::Idle, pool(&[1, 2, 3]))]);
  assert!(!fmt.pack_idle_sessions());
  assert_eq!(ids(&fmt.sessions[0]), vec![1, 2, 3]);
}
