import React from 'react';
import { JobState, MediaKind } from '../../api/contracts';

interface BadgeProps {
  type: 'kind' | 'state' | 'generic';
  value: MediaKind | JobState | string;
  className?: string;
}

export const Badge: React.FC<BadgeProps> = ({ type, value, className = '' }) => {
  let badgeClass = 'badge';

  if (type === 'kind') {
    badgeClass += ` badge-${value}`;
  } else if (type === 'state') {
    badgeClass += ` badge-${value}`;
  }

  return (
    <span className={`${badgeClass} ${className}`.trim()} data-testid={`badge-${value}`}>
      {value}
    </span>
  );
};
