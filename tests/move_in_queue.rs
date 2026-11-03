use qbot::models::{Format, Session, SessionPlayer, SessionStatus, Team};
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
fn buffer_pulls_waiting_player_into_ready_game() {
  let mut fmt = fmt_with(vec![Session::new(SessionStatus::Hot, pool(&[1, 2, 3, 4, 5, 6, 7, 8])), Session::new(SessionStatus::Idle, pool(&[9, 10, 11]))]);
  assert_eq!(fmt.move_in_queue(UserId::new(10), true), Some(true));
  assert_eq!(ids(&fmt.sessions[0]), vec![10, 1, 2, 3, 4, 5, 6, 7]);
  assert_eq!(fmt.sessions[1].status, SessionStatus::Idle);
  assert_eq!(ids(&fmt.sessions[1]), vec![8, 9, 11]);
}

#[test]
fn fatkid_pushes_ready_player_into_next_queue() {
  let mut fmt = fmt_with(vec![Session::new(SessionStatus::Hot, pool(&[1, 2, 3, 4, 5, 6, 7, 8])), Session::new(SessionStatus::Idle, pool(&[9, 10, 11]))]);
  assert_eq!(fmt.move_in_queue(UserId::new(3), false), Some(true));
  assert_eq!(ids(&fmt.sessions[0]), vec![1, 2, 4, 5, 6, 7, 8, 9]);
  assert_eq!(ids(&fmt.sessions[1]), vec![10, 11, 3]);
}

#[test]
fn buffer_in_idle_only_queue_keeps_ready_set() {
  let mut fmt = fmt_with(vec![Session::new(SessionStatus::Idle, pool(&[1, 2, 3, 4, 5]))]);
  assert_eq!(fmt.move_in_queue(UserId::new(5), true), Some(false));
  assert_eq!(fmt.sessions.len(), 1);
  assert_eq!(ids(&fmt.sessions[0]), vec![5, 1, 2, 3, 4]);
}

#[test]
fn fatkid_without_waiting_queue_keeps_ready_game_and_adds_no_queue() {
  let mut fmt = fmt_with(vec![Session::new(SessionStatus::Hot, pool(&[1, 2, 3, 4, 5, 6, 7, 8]))]);
  assert_eq!(fmt.move_in_queue(UserId::new(3), false), Some(false));
  assert_eq!(fmt.sessions.len(), 1);
  assert_eq!(ids(&fmt.sessions[0]), vec![1, 2, 4, 5, 6, 7, 8, 3]);
}

#[test]
fn unqueued_or_unknown_player_is_not_mutated() {
  let mut fmt = fmt_with(vec![Session::new(SessionStatus::Live, pool(&[1, 2, 3, 4, 5, 6, 7, 8])), Session::new(SessionStatus::Idle, pool(&[9, 10]))]);
  assert_eq!(fmt.move_in_queue(UserId::new(3), true), None);
  assert_eq!(fmt.move_in_queue(UserId::new(99), false), None);
  assert_eq!(fmt.sessions[0].status, SessionStatus::Live);
  assert_eq!(ids(&fmt.sessions[0]), vec![1, 2, 3, 4, 5, 6, 7, 8]);
  assert_eq!(ids(&fmt.sessions[1]), vec![9, 10]);
}

#[test]
fn player_pushed_out_of_ready_game_loses_team() {
  let mut hot = pool(&[1, 2, 3, 4, 5, 6, 7, 8]);
  for p in &mut hot {
    p.team = Some(Team::Red);
  }
  let mut fmt = fmt_with(vec![Session::new(SessionStatus::Hot, hot), Session::new(SessionStatus::Idle, pool(&[9, 10, 11]))]);
  assert_eq!(fmt.move_in_queue(UserId::new(10), true), Some(true));
  let displaced = fmt.sessions[1].pool.iter().find(|p| p.player.user_id == UserId::new(8)).unwrap();
  assert_eq!(displaced.team, None);
  let kept = fmt.sessions[0].pool.iter().find(|p| p.player.user_id == UserId::new(7)).unwrap();
  assert_eq!(kept.team, Some(Team::Red));
}
