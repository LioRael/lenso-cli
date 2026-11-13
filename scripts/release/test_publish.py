"""Release confirmation must tolerate a delayed tag without repeating publication."""
import unittest
from unittest.mock import patch
import publish

class PublicationConfirmation(unittest.TestCase):
    def test_delayed_tag_is_rechecked_with_reads_only(self):
        release = {'id': 1, 'tag_name': 'v1.1.0', 'draft': False}
        with patch.object(publish, 'api', return_value=release) as api, \
             patch.object(publish, 'check_tag', side_effect=[False, False, True]), \
             patch.object(publish.time, 'sleep') as sleep:
            self.assertEqual(publish.confirm_publication('owner/repo', 1, 'v1.1.0', 'commit'), release)
            self.assertEqual(api.call_count, 3)
            self.assertTrue(all(call.args == ('owner/repo', 'releases/1') and not call.kwargs for call in api.call_args_list))
            self.assertEqual(sleep.call_count, 2)

    def test_confirmation_is_bounded_and_rejects_changed_tags(self):
        with patch.object(publish, 'api', return_value={'tag_name': 'v1.1.0', 'draft': False}), \
             patch.object(publish, 'check_tag', return_value=False), \
             patch.object(publish.time, 'sleep') as sleep:
            with self.assertRaisesRegex(RuntimeError, 'inspect its state'):
                publish.confirm_publication('owner/repo', 1, 'v1.1.0', 'commit')
            self.assertEqual(sleep.call_count, 4)
        with patch.object(publish, 'api', return_value={'tag_name': 'other', 'draft': False}):
            with self.assertRaisesRegex(RuntimeError, 'tag changed'):
                publish.confirm_publication('owner/repo', 1, 'v1.1.0', 'commit')

if __name__ == '__main__':
    unittest.main()
